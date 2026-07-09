use crate::{ast::*, parser};
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use std::collections::HashSet;
use std::path::Path;

const EXCLUDED: &[&str] = &["wl_display", "wl_callback", "wl_registry"];

pub fn generate<P: AsRef<Path>>(paths: &[P]) {
    let protocols: Vec<_> = paths.iter().map(|p| parser::parse_xml(p)).collect();
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let out = |name: &str| std::path::Path::new(&out_dir).join(name);

    let interfaces: Vec<&Interface> = protocols
        .iter()
        .flat_map(|p| p.interfaces())
        .filter(|i| !EXCLUDED.contains(&i.name.as_str()))
        .collect();

    let excluded_idents: Vec<Ident> = EXCLUDED.iter().map(|n| type_ident(n)).collect();

    // ── generated_shared.rs ───────────────────────────────────────────────────
    // Common imports, read helpers, interface structs, enum defs.
    // Included unconditionally; its definitions are in scope for whichever of
    // generated_client.rs / generated_server.rs follows.

    let read_helpers = gen_read_helpers();
    let shared_items: Vec<TokenStream> = interfaces.iter().map(|i| gen_shared(i)).collect();

    let shared = quote! {
        #[allow(unused_imports, dead_code, unused_variables, unused_mut, non_camel_case_types)]
        use crate::{Handle, Interface, ObjectId, RawWaylandEvent, Wayland};
        use super::manual::{#(#excluded_idents),*};
        use app::prelude::*;
        use bitflags::bitflags;

        #read_helpers
        #(#shared_items)*
    };
    write_formatted(out("generated_shared.rs"), shared);

    // ── generated_client.rs ───────────────────────────────────────────────────
    // XxxEvent enums + parse, Handle<T> request-sending methods, client_module().
    // No use statements — shared definitions are already in scope.

    let client_items: Vec<TokenStream> = interfaces.iter().map(|i| gen_client(i)).collect();
    let client_mod = gen_client_module(&interfaces);

    let client = quote! {
        #(#client_items)*
        #client_mod
    };
    write_formatted(out("generated_client.rs"), client);

    // ── generated_server.rs ───────────────────────────────────────────────────
    // XxxRequest enums + parse, Handle<T> event-sending methods, server_dispatch_module().
    // Only adds server-specific imports on top of what shared already provided.

    let server_items: Vec<TokenStream> = interfaces.iter().map(|i| gen_server(i)).collect();
    let server_mod = gen_server_module(&interfaces);

    let server = quote! {
        use crate::server::{ClientRawEvent, WaylandServer};
        use app::RegisteredModule;

        #(#server_items)*
        #server_mod
    };
    write_formatted(out("generated_server.rs"), server);
}

fn write_formatted(path: std::path::PathBuf, code: TokenStream) {
    let formatted =
        prettyplease::unparse(&syn::parse2(code).expect("generated code is not valid Rust"));
    std::fs::write(path, formatted).expect("failed to write generated file");
}

// ── name helpers ──────────────────────────────────────────────────────────────

fn to_pascal(s: &str) -> String {
    s.split('_')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect()
}

fn variant_name(s: &str) -> String {
    let p = to_pascal(s);
    if p.chars()
        .next()
        .map(|c| c.is_ascii_digit())
        .unwrap_or(false)
    {
        format!("N{p}")
    } else {
        p
    }
}

fn id(s: &str) -> Ident {
    let span = Span::call_site();
    match s {
        "type" | "loop" | "use" | "impl" | "fn" | "let" | "mut" | "ref" | "move" | "return"
        | "where" | "in" | "match" | "if" | "else" | "while" | "for" | "struct" | "enum"
        | "trait" | "pub" | "mod" | "self" | "super" | "as" | "break" | "continue" | "const"
        | "static" | "unsafe" | "extern" | "async" | "await" => Ident::new_raw(s, span),
        _ => Ident::new(s, span),
    }
}

fn type_ident(iface: &str) -> Ident {
    id(&to_pascal(iface))
}

fn resolve_enum_ident(iface_name: &str, enum_attr: &str) -> Ident {
    if let Some((i, e)) = enum_attr.split_once('.') {
        id(&format!("{}{}", to_pascal(i), to_pascal(e)))
    } else {
        id(&format!(
            "{}{}",
            to_pascal(iface_name),
            to_pascal(enum_attr)
        ))
    }
}

// ── doc-comment helpers ───────────────────────────────────────────────────────

fn doc_comment(desc: Option<&Description>) -> TokenStream {
    let Some(d) = desc else {
        return quote! {};
    };
    let mut attrs = TokenStream::new();
    if let Some(ref s) = d.summary {
        let s = format!(" {}", s.trim());
        if !s.trim().is_empty() {
            attrs.extend(quote! { #[doc = #s] });
        }
    }
    let body = d.text.trim().to_string();
    if !body.is_empty() {
        attrs.extend(quote! { #[doc = ""] });
        for line in body.lines() {
            let line = format!(" {}", line.trim());
            attrs.extend(quote! { #[doc = #line] });
        }
    }
    attrs
}

fn doc_summary(summary: Option<&str>) -> TokenStream {
    let Some(s) = summary else {
        return quote! {};
    };
    let s = s.trim();
    if s.is_empty() {
        return quote! {};
    }
    let s = format!(" {s}");
    quote! { #[doc = #s] }
}

// ── read helpers (emitted into client and server files) ───────────────────────

fn gen_read_helpers() -> TokenStream {
    quote! {
        fn read_u32(data: &[u8], o: &mut usize) -> Option<u32> {
            let v = data.get(*o..*o + 4)?;
            *o += 4;
            Some(u32::from_ne_bytes(v.try_into().unwrap()))
        }

        fn read_i32(data: &[u8], o: &mut usize) -> Option<i32> {
            read_u32(data, o).map(|v| v as i32)
        }

        fn read_str(data: &[u8], o: &mut usize) -> Option<String> {
            let len = read_u32(data, o)? as usize;
            let padded = (len + 3) & !3;
            let raw = data.get(*o..*o + padded)?;
            *o += padded;
            let s = std::str::from_utf8(raw.get(..len.saturating_sub(1))?).ok()?;
            Some(s.to_owned())
        }

        fn read_str_opt(data: &[u8], o: &mut usize) -> Option<Option<String>> {
            let len = read_u32(data, o)? as usize;
            if len == 0 { return Some(None); }
            let padded = (len + 3) & !3;
            let raw = data.get(*o..*o + padded)?;
            *o += padded;
            let s = std::str::from_utf8(raw.get(..len.saturating_sub(1))?).ok()?;
            Some(Some(s.to_owned()))
        }

        fn read_array(data: &[u8], o: &mut usize) -> Option<Vec<u8>> {
            let len = read_u32(data, o)? as usize;
            let padded = (len + 3) & !3;
            let raw = data.get(*o..*o + padded)?;
            *o += padded;
            Some(raw[..len].to_vec())
        }
    }
}

// ── shared: interface struct + enum defs ──────────────────────────────────────

fn gen_shared(iface: &Interface) -> TokenStream {
    let tname = type_ident(&iface.name);
    let iface_name_str = &iface.name;
    let version = iface.version;
    let iface_doc = doc_comment(iface.description());

    let enum_defs: Vec<TokenStream> = iface
        .enums()
        .map(|e| gen_enum_def(&iface.name, e))
        .collect();

    quote! {
        #iface_doc
        #[derive(Debug)]
        pub struct #tname;

        impl Interface for #tname {
            const NAME: &'static str = #iface_name_str;
            const VERSION: u32 = #version;
        }

        #(#enum_defs)*
    }
}

// ── client: XxxEvent enum + parse + Handle<T> request methods ─────────────────

fn gen_client(iface: &Interface) -> TokenStream {
    let tname = type_ident(&iface.name);
    let events: Vec<&Message> = iface.events().collect();
    let requests: Vec<&Message> = iface.requests().collect();

    let event_tokens = if !events.is_empty() {
        gen_event_enum(&iface.name, &tname, &events)
    } else {
        quote! {}
    };

    let handle_tokens = if !requests.is_empty() {
        gen_client_handle_impl(&iface.name, &tname, &requests)
    } else {
        quote! {}
    };

    quote! {
        #event_tokens
        #handle_tokens
    }
}

// ── server: XxxRequest enum + parse + Handle<T> event methods ─────────────────

fn gen_server(iface: &Interface) -> TokenStream {
    let tname = type_ident(&iface.name);
    let requests: Vec<&Message> = iface.requests().collect();
    let events: Vec<&Message> = iface.events().collect();

    let request_tokens = if !requests.is_empty() {
        gen_request_enum(&iface.name, &tname, &requests)
    } else {
        quote! {}
    };

    let handle_tokens = if !events.is_empty() {
        gen_server_handle_impl(&iface.name, &tname, &events)
    } else {
        quote! {}
    };

    quote! {
        #request_tokens
        #handle_tokens
    }
}

// ── enum definitions ──────────────────────────────────────────────────────────

fn gen_enum_def(iface_name: &str, en: &EnumDef) -> TokenStream {
    let ename = resolve_enum_ident(iface_name, &en.name);
    let enum_doc = doc_comment(en.description.as_ref());
    let mut seen = HashSet::new();

    if en.bitfield {
        let entries: Vec<TokenStream> = en
            .entries
            .iter()
            .filter(|e| seen.insert(e.value.clone()))
            .map(|e| {
                let vname = id(&variant_name(&e.name));
                let val: TokenStream = e.value.parse().unwrap();
                let entry_doc = doc_summary(e.summary.as_deref());
                quote! {
                    #entry_doc
                    const #vname = #val;
                }
            })
            .collect();

        quote! {
            bitflags! {
                #enum_doc
                #[derive(Debug, Clone, Copy, PartialEq, Eq)]
                pub struct #ename: u32 {
                    #(#entries)*
                }
            }

            impl TryFrom<u32> for #ename {
                type Error = u32;
                fn try_from(v: u32) -> Result<Self, u32> { Ok(Self::from_bits_truncate(v)) }
            }

            impl From<#ename> for u32 {
                fn from(v: #ename) -> u32 { v.bits() }
            }
        }
    } else {
        let mut unique_entries: Vec<&EnumEntry> = Vec::new();
        for e in &en.entries {
            if seen.insert(e.value.clone()) {
                unique_entries.push(e);
            }
        }

        let variants: Vec<TokenStream> = unique_entries
            .iter()
            .map(|e| {
                let vname = id(&variant_name(&e.name));
                let entry_doc = doc_summary(e.summary.as_deref());
                quote! {
                    #entry_doc
                    #vname,
                }
            })
            .collect();

        let try_from_arms: Vec<TokenStream> = unique_entries
            .iter()
            .map(|e| {
                let vname = id(&variant_name(&e.name));
                let val: TokenStream = e.value.parse().unwrap();
                quote! { #val => Ok(Self::#vname), }
            })
            .collect();

        let into_u32_arms: Vec<TokenStream> = unique_entries
            .iter()
            .map(|e| {
                let vname = id(&variant_name(&e.name));
                let val: TokenStream = e.value.parse().unwrap();
                quote! { #ename::#vname => #val, }
            })
            .collect();

        quote! {
            #enum_doc
            #[derive(Debug, Clone, Copy, PartialEq, Eq)]
            pub enum #ename {
                #(#variants)*
            }

            impl TryFrom<u32> for #ename {
                type Error = u32;
                fn try_from(v: u32) -> Result<Self, u32> {
                    match v {
                        #(#try_from_arms)*
                        other => Err(other),
                    }
                }
            }

            impl From<#ename> for u32 {
                fn from(v: #ename) -> u32 {
                    match v {
                        #(#into_u32_arms)*
                    }
                }
            }
        }
    }
}

// ── XxxRequest enum + parse ───────────────────────────────────────────────────

fn gen_request_enum(iface_name: &str, tname: &Ident, requests: &[&Message]) -> TokenStream {
    let ename = id(&format!("{tname}Request"));

    let variants: Vec<TokenStream> = requests
        .iter()
        .map(|req| {
            let vname = id(&variant_name(&req.name));
            let variant_doc = doc_comment(req.description.as_ref());
            let fields: Vec<TokenStream> = req
                .args
                .iter()
                .map(|a| {
                    let fname = id(&a.name);
                    let ftype = parsed_field_type(iface_name, a);
                    let field_doc = doc_summary(a.summary.as_deref());
                    quote! { #field_doc #fname: #ftype }
                })
                .collect();
            quote! {
                #variant_doc
                #vname { sender: Handle<#tname>, #(#fields),* },
            }
        })
        .collect();

    let arms: Vec<TokenStream> = requests
        .iter()
        .enumerate()
        .map(|(opcode, req)| {
            let opcode = opcode as u32;
            let vname = id(&variant_name(&req.name));
            let stmts = gen_parse_stmts(iface_name, &req.args);
            let field_names: Vec<Ident> = req.args.iter().map(|a| id(&a.name)).collect();
            let ret = quote! { Some(#ename::#vname { sender: sender.clone(), #(#field_names),* }) };
            quote! {
                #opcode => {
                    #stmts
                    #ret
                }
            }
        })
        .collect();

    quote! {
        #[derive(Debug)]
        pub enum #ename {
            #(#variants)*
        }

        impl Event for #ename {}

        impl #ename {
            pub fn parse(event: &RawWaylandEvent, wayland: &mut Wayland) -> Option<Self> {
                let sender = wayland.get_handle::<#tname>(event.object_id)?;
                let data = &event.data;
                let mut o = 0usize;
                match event.opcode {
                    #(#arms)*
                    _ => None,
                }
            }
        }
    }
}

// ── XxxEvent enum + parse ─────────────────────────────────────────────────────

fn gen_event_enum(iface_name: &str, tname: &Ident, events: &[&Message]) -> TokenStream {
    let ename = id(&format!("{tname}Event"));

    let variants: Vec<TokenStream> = events
        .iter()
        .map(|ev| {
            let vname = id(&variant_name(&ev.name));
            let variant_doc = doc_comment(ev.description.as_ref());
            let fields: Vec<TokenStream> = ev
                .args
                .iter()
                .map(|a| {
                    let fname = id(&a.name);
                    let ftype = parsed_field_type(iface_name, a);
                    let field_doc = doc_summary(a.summary.as_deref());
                    quote! { #field_doc #fname: #ftype }
                })
                .collect();
            quote! {
                #variant_doc
                #vname { sender: Handle<#tname>, #(#fields),* },
            }
        })
        .collect();

    let arms: Vec<TokenStream> = events
        .iter()
        .enumerate()
        .map(|(opcode, ev)| {
            let opcode = opcode as u32;
            let vname = id(&variant_name(&ev.name));
            let stmts = gen_parse_stmts(iface_name, &ev.args);
            let field_names: Vec<Ident> = ev.args.iter().map(|a| id(&a.name)).collect();
            let ret = quote! { Some(#ename::#vname { sender: sender.clone(), #(#field_names),* }) };
            quote! {
                #opcode => {
                    #stmts
                    #ret
                }
            }
        })
        .collect();

    quote! {
        #[derive(Debug)]
        pub enum #ename {
            #(#variants)*
        }

        impl Event for #ename {}

        impl #ename {
            pub fn parse(event: &RawWaylandEvent, wayland: &mut Wayland) -> Option<Self> {
                let sender = wayland.get_handle::<#tname>(event.object_id)?;
                let data = &event.data;
                let mut o = 0usize;
                match event.opcode {
                    #(#arms)*
                    _ => None,
                }
            }
        }
    }
}

// ── client: Handle<T> request-sending methods ─────────────────────────────────

fn gen_client_handle_impl(iface_name: &str, tname: &Ident, requests: &[&Message]) -> TokenStream {
    let methods: Vec<TokenStream> = requests
        .iter()
        .enumerate()
        .map(|(opcode, req)| gen_send_method(iface_name, opcode as u16, req, true))
        .collect();

    quote! {
        impl Handle<#tname> {
            #(#methods)*
        }
    }
}

// ── server: Handle<T> event-sending methods ───────────────────────────────────

fn gen_server_handle_impl(iface_name: &str, tname: &Ident, events: &[&Message]) -> TokenStream {
    let methods: Vec<TokenStream> = events
        .iter()
        .enumerate()
        .map(|(opcode, ev)| gen_send_method(iface_name, opcode as u16, ev, false))
        .collect();

    quote! {
        impl Handle<#tname> {
            #(#methods)*
        }
    }
}

// Generates a wire-send method for either a client request or a server event.
// `is_request` controls whether new_id args use alloc_handle (true → client allocates,
// false → server allocates from 0xFF000000+ range seeded in Wayland::new_server).
fn gen_send_method(iface_name: &str, opcode: u16, msg: &Message, is_request: bool) -> TokenStream {
    let mname = id(&msg.name);

    let new_id_arg = msg
        .args
        .iter()
        .find(|a| a.arg_type == ArgType::NewId && a.interface.is_some());

    let params: Vec<TokenStream> = msg
        .args
        .iter()
        .filter(|a| a.arg_type != ArgType::NewId)
        .map(|a| {
            let pname = id(&a.name);
            let ptype = send_param_type(iface_name, a);
            quote! { #pname: #ptype }
        })
        .collect();

    let alloc = new_id_arg.map(|a| {
        let fname = id(&a.name);
        let fname_id = id(&format!("{}_id", a.name));
        let t = type_ident(a.interface.as_ref().unwrap());
        quote! {
            let #fname: Handle<#t> = self.proxy.alloc_handle();
            let #fname_id = #fname.object_id().expect("just allocated").0;
        }
    });

    let has_body = msg.args.iter().any(|a| a.arg_type != ArgType::Fd);
    let has_fds = msg.args.iter().any(|a| a.arg_type == ArgType::Fd);
    let encode: Vec<TokenStream> = msg.args.iter().map(gen_encode_arg).collect();

    let write = if has_body || has_fds {
        let body_init = has_body.then(|| quote! { let mut body: Vec<u8> = Vec::new(); });
        let fds_init = has_fds.then(|| {
            quote! { let mut fds: Vec<::std::os::fd::BorrowedFd<'_>> = Vec::new(); }
        });
        let body_ref = if has_body { quote! { &body } } else { quote! { &[] } };
        let fds_ref = if has_fds { quote! { &fds } } else { quote! { &[] } };
        quote! {
            #body_init
            #fds_init
            #(#encode)*
            self.proxy.write_raw(sender_id, #opcode, #body_ref, #fds_ref);
        }
    } else {
        quote! { self.proxy.write_raw(sender_id, #opcode, &[], &[]); }
    };

    let ret_type = new_id_arg.map(|a| {
        let t = type_ident(a.interface.as_ref().unwrap());
        quote! { -> Handle<#t> }
    });

    let ret_val = new_id_arg.map(|a| {
        let fname = id(&a.name);
        quote! { #fname }
    });

    let arg_docs: Vec<TokenStream> = msg
        .args
        .iter()
        .filter(|a| a.arg_type != ArgType::NewId && a.summary.is_some())
        .map(|a| {
            let name = &a.name;
            let summary = format!(
                " * `{}` - {}",
                name,
                a.summary.as_deref().unwrap_or("").trim()
            );
            quote! { #[doc = #summary] #[doc = ""] }
        })
        .collect();

    let args_section: TokenStream = if arg_docs.is_empty() {
        quote! {}
    } else {
        quote! {
            #[doc = ""]
            #[doc = " # Arguments"]
            #[doc = ""]
            #(#arg_docs)*
        }
    };

    let method_doc = doc_comment(msg.description.as_ref());
    let _ = is_request; // both sides use identical wire encoding

    quote! {
        #method_doc
        #args_section
        pub fn #mname(&self, #(#params),*) #ret_type {
            #alloc
            let sender_id = self.object_id().expect("dead handle").0;
            #write
            #ret_val
        }
    }
}

// ── parse statements ──────────────────────────────────────────────────────────

fn gen_parse_stmts(iface_name: &str, args: &[Arg]) -> TokenStream {
    let stmts: Vec<TokenStream> = args.iter().map(|a| gen_parse_stmt(iface_name, a)).collect();
    quote! { #(#stmts)* }
}

fn gen_parse_stmt(iface_name: &str, arg: &Arg) -> TokenStream {
    let fname = id(&arg.name);

    match arg.arg_type {
        ArgType::Fd => quote! {
            let #fname = wayland.take_fd()?;
        },
        ArgType::Int | ArgType::Fixed => quote! {
            let #fname = read_i32(data, &mut o)?;
        },
        ArgType::Uint => {
            if let Some(ref e) = arg.enum_type {
                let etype = resolve_enum_ident(iface_name, e);
                quote! { let #fname = #etype::try_from(read_u32(data, &mut o)?).ok()?; }
            } else {
                quote! { let #fname = read_u32(data, &mut o)?; }
            }
        }
        ArgType::String => {
            if arg.allow_null {
                quote! { let #fname = read_str_opt(data, &mut o)?; }
            } else {
                quote! { let #fname = read_str(data, &mut o)?; }
            }
        }
        ArgType::Array => quote! { let #fname = read_array(data, &mut o)?; },
        ArgType::NewId => {
            let raw = id(&format!("{}_raw", arg.name));
            if let Some(ref iface) = arg.interface {
                let t = type_ident(iface);
                quote! {
                    let #raw = read_u32(data, &mut o)?;
                    let #fname = wayland.new_handle::<#t>(ObjectId(#raw));
                }
            } else {
                let _iface_var = id(&format!("_{}_iface", arg.name));
                let _ver_var = id(&format!("_{}_ver", arg.name));
                quote! {
                    let #raw = read_u32(data, &mut o)?;
                    let #_iface_var = read_str(data, &mut o)?;
                    let #_ver_var = read_u32(data, &mut o)?;
                    let #fname = ObjectId(#raw);
                }
            }
        }
        ArgType::Object => {
            let raw = id(&format!("{}_raw", arg.name));
            if let Some(ref iface) = arg.interface {
                let t = type_ident(iface);
                if arg.allow_null {
                    quote! {
                        let #raw = read_u32(data, &mut o)?;
                        let #fname = if #raw == 0 {
                            None
                        } else {
                            Some(wayland.get_handle::<#t>(ObjectId(#raw))?)
                        };
                    }
                } else {
                    quote! {
                        let #raw = read_u32(data, &mut o)?;
                        let #fname = wayland.get_handle::<#t>(ObjectId(#raw))?;
                    }
                }
            } else if arg.allow_null {
                quote! {
                    let #raw = read_u32(data, &mut o)?;
                    let #fname = if #raw == 0 { None } else { Some(ObjectId(#raw)) };
                }
            } else {
                quote! {
                    let #raw = read_u32(data, &mut o)?;
                    let #fname = ObjectId(#raw);
                }
            }
        }
    }
}

// ── encode args ───────────────────────────────────────────────────────────────

fn gen_encode_arg(arg: &Arg) -> TokenStream {
    let fname = id(&arg.name);

    match arg.arg_type {
        ArgType::Fd => {
            quote! { fds.push(#fname); }
        }
        ArgType::NewId => {
            let fname_id = id(&format!("{}_id", arg.name));
            quote! { body.extend_from_slice(&#fname_id.to_ne_bytes()); }
        }
        ArgType::Int | ArgType::Fixed => {
            quote! { body.extend_from_slice(&(#fname as u32).to_ne_bytes()); }
        }
        ArgType::Uint => {
            quote! { body.extend_from_slice(&u32::from(#fname).to_ne_bytes()); }
        }
        ArgType::String => {
            if arg.allow_null {
                quote! {
                    match #fname {
                        Some(s) => crate::helper::encode_string(&mut body, s),
                        None => body.extend_from_slice(&0u32.to_ne_bytes()),
                    }
                }
            } else {
                quote! { crate::helper::encode_string(&mut body, #fname); }
            }
        }
        ArgType::Array => {
            quote! {
                body.extend_from_slice(&(#fname.len() as u32).to_ne_bytes());
                body.extend_from_slice(#fname);
                let _pad = (4 - (#fname.len() % 4)) % 4;
                for _ in 0.._pad { body.push(0); }
            }
        }
        ArgType::Object => {
            if arg.interface.is_some() {
                if arg.allow_null {
                    quote! {
                        body.extend_from_slice(
                            &#fname.map(|h| h.object_id().expect("dead handle").0).unwrap_or(0).to_ne_bytes()
                        );
                    }
                } else {
                    quote! {
                        body.extend_from_slice(&#fname.object_id().expect("dead handle").0.to_ne_bytes());
                    }
                }
            } else if arg.allow_null {
                quote! {
                    body.extend_from_slice(&#fname.map(|id| id.0).unwrap_or(0).to_ne_bytes());
                }
            } else {
                quote! { body.extend_from_slice(&#fname.0.to_ne_bytes()); }
            }
        }
    }
}

// ── type helpers ──────────────────────────────────────────────────────────────

fn parsed_field_type(iface_name: &str, arg: &Arg) -> TokenStream {
    match arg.arg_type {
        ArgType::Int | ArgType::Fixed => quote! { i32 },
        ArgType::Uint => {
            if let Some(ref e) = arg.enum_type {
                let t = resolve_enum_ident(iface_name, e);
                quote! { #t }
            } else {
                quote! { u32 }
            }
        }
        ArgType::String => {
            if arg.allow_null {
                quote! { Option<String> }
            } else {
                quote! { String }
            }
        }
        ArgType::Array => quote! { Vec<u8> },
        ArgType::Fd => quote! { ::std::os::fd::OwnedFd },
        ArgType::NewId => {
            if let Some(ref iface) = arg.interface {
                let t = type_ident(iface);
                quote! { Handle<#t> }
            } else {
                quote! { ObjectId }
            }
        }
        ArgType::Object => {
            if let Some(ref iface) = arg.interface {
                let t = type_ident(iface);
                if arg.allow_null {
                    quote! { Option<Handle<#t>> }
                } else {
                    quote! { Handle<#t> }
                }
            } else if arg.allow_null {
                quote! { Option<ObjectId> }
            } else {
                quote! { ObjectId }
            }
        }
    }
}

fn send_param_type(iface_name: &str, arg: &Arg) -> TokenStream {
    match arg.arg_type {
        ArgType::Int | ArgType::Fixed => quote! { i32 },
        ArgType::Uint => {
            if let Some(ref e) = arg.enum_type {
                let t = resolve_enum_ident(iface_name, e);
                quote! { #t }
            } else {
                quote! { u32 }
            }
        }
        ArgType::String => {
            if arg.allow_null {
                quote! { Option<&str> }
            } else {
                quote! { &str }
            }
        }
        ArgType::Array => quote! { &[u8] },
        ArgType::Fd => quote! { ::std::os::fd::BorrowedFd<'_> },
        ArgType::NewId => quote! { () },
        ArgType::Object => {
            if let Some(ref iface) = arg.interface {
                let t = type_ident(iface);
                if arg.allow_null {
                    quote! { Option<&Handle<#t>> }
                } else {
                    quote! { &Handle<#t> }
                }
            } else if arg.allow_null {
                quote! { Option<ObjectId> }
            } else {
                quote! { ObjectId }
            }
        }
    }
}

// ── client_module ─────────────────────────────────────────────────────────────

fn gen_client_module(interfaces: &[&Interface]) -> TokenStream {
    let handlers: Vec<TokenStream> = interfaces
        .iter()
        .filter(|i| i.events().next().is_some())
        .map(|i| {
            let tname = type_ident(&i.name);
            let ename = id(&format!("{tname}Event"));
            quote! {
                .on(|wayland: &mut Wayland, raw: &RawWaylandEvent| {
                    if wayland.get_interface(raw.object_id) == Some(#tname::NAME) {
                        #ename::parse(raw, wayland)
                    } else {
                        None
                    }
                })
            }
        })
        .collect();

    quote! {
        pub fn client_module<S>() -> impl app::RegisteredModule<Wayland, S> {
            let m = app::Module::new();
            let m = m #(#handlers)*;
            m
        }
    }
}

// ── server_dispatch_module ────────────────────────────────────────────────────

fn gen_server_module(interfaces: &[&Interface]) -> TokenStream {
    let handlers: Vec<TokenStream> = interfaces
        .iter()
        .filter(|i| i.requests().next().is_some())
        .map(|i| {
            let tname = type_ident(&i.name);
            let rname = id(&format!("{tname}Request"));
            quote! {
                .on(|server: &mut WaylandServer, ev: &ClientRawEvent| {
                    let mut inner = server.data.borrow_mut();
                    let client = inner.clients.get_mut(&ev.client_id)?;
                    if client.conn.get_interface(ev.raw.object_id) == Some(#tname::NAME) {
                        #rname::parse(&ev.raw, &mut client.conn)
                    } else {
                        None
                    }
                })
            }
        })
        .collect();

    quote! {
        pub fn server_dispatch_module<S>() -> impl RegisteredModule<WaylandServer, S> {
            let m = app::Module::new();
            let m = m #(#handlers)*;
            m
        }
    }
}
