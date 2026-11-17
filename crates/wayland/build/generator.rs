use std::collections::HashSet;
use std::path::Path;

use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;

use crate::ast::*;
use crate::parser;

const EXCLUDED: &[&str] = &["wl_display", "wl_callback", "wl_registry"];

pub fn generate<P: AsRef<Path>>(paths: &[P]) {
    let protocols: Vec<Protocol> = paths.iter().map(|p| parser::parse_xml(p)).collect();
    let interfaces: Vec<&Interface> = protocols
        .iter()
        .flat_map(|p| p.interfaces())
        .filter(|i| !EXCLUDED.contains(&i.name.as_str()))
        .collect();

    let items: Vec<TokenStream> = interfaces.iter().map(|i| gen_interface(i)).collect();
    let file = quote! {
        use std::collections::VecDeque;
        use std::os::fd::{BorrowedFd, OwnedFd};
        use app::prelude::*;
        use bitflags::bitflags;
        #[allow(unused_imports)]
        use crate::display::{WlCallback, WlDisplay, WlRegistry};
        use crate::wire::Reader;
        use crate::{Info, Interface, ObjectId, Wayland};
        #(#items)*
    };
    let out = Path::new(&std::env::var("OUT_DIR").unwrap()).join("generated.rs");
    let formatted =
        prettyplease::unparse(&syn::parse2(file).expect("generated code is not valid Rust"));
    std::fs::write(out, formatted).expect("write generated.rs");
}

// ── names ────────────────────────────────────────────────────────────────────

fn to_pascal(s: &str) -> String {
    s.split('_')
        .map(|part| {
            let mut c = part.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            }
        })
        .collect()
}

fn variant_name(s: &str) -> String {
    let p = to_pascal(s);
    if p.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        format!("N{p}")
    } else {
        p
    }
}

const KEYWORDS: &[&str] = &[
    "type", "loop", "use", "impl", "fn", "let", "mut", "ref", "move", "return", "where", "in",
    "match", "if", "else", "while", "for", "struct", "enum", "trait", "pub", "mod", "self",
    "super", "as", "break", "continue", "const", "static", "unsafe", "extern", "async", "await",
];

fn id(s: &str) -> Ident {
    if KEYWORDS.contains(&s) {
        Ident::new_raw(s, Span::call_site())
    } else {
        Ident::new(s, Span::call_site())
    }
}

fn type_ident(iface: &str) -> Ident {
    id(&to_pascal(iface))
}

/// The sender field: the interface name without its namespace prefix
/// and version suffix. `wl_surface` is `surface`, `zwlr_layer_surface_v1`
/// is `layer_surface`, `xdg_wm_base` is `wm_base`.
fn sender_name(iface: &str) -> String {
    let mut s = iface;
    for prefix in ["wl_", "xdg_", "zwlr_", "zwp_", "ext_", "wp_", "mx_"] {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = rest;
            break;
        }
    }
    if let Some(i) = s.rfind("_v")
        && s[i + 2..].chars().all(|c| c.is_ascii_digit())
        && !s[i + 2..].is_empty()
    {
        s = &s[..i];
    }
    s.to_string()
}

fn enum_ident(iface: &str, attr: &str) -> Ident {
    match attr.split_once('.') {
        Some((i, e)) => id(&format!("{}{}", to_pascal(i), to_pascal(e))),
        None => id(&format!("{}{}", to_pascal(iface), to_pascal(attr))),
    }
}

fn doc(desc: Option<&Description>) -> TokenStream {
    let Some(d) = desc else { return quote! {} };
    let mut attrs = TokenStream::new();
    if let Some(s) = d
        .summary
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let s = format!(" {s}");
        attrs.extend(quote! { #[doc = #s] });
    }
    let body = d.text.trim();
    if !body.is_empty() {
        attrs.extend(quote! { #[doc = ""] });
        for line in body.lines() {
            let line = format!(" {}", line.trim());
            attrs.extend(quote! { #[doc = #line] });
        }
    }
    attrs
}

fn summary(s: Option<&str>) -> TokenStream {
    match s.map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => {
            let s = format!(" {s}");
            quote! { #[doc = #s] }
        }
        None => quote! {},
    }
}

// ── one interface ────────────────────────────────────────────────────────────

fn gen_interface(iface: &Interface) -> TokenStream {
    let tname = type_ident(&iface.name);
    let name = &iface.name;
    let version = iface.version;
    let idoc = doc(iface.description());
    let dispatch = id(&format!("dispatch_{}", iface.name));
    let enums: Vec<TokenStream> = iface.enums().map(|e| gen_enum(&iface.name, e)).collect();
    let events: Vec<&Message> = iface.events().collect();
    let requests: Vec<&Message> = iface.requests().collect();

    let event_items = if events.is_empty() {
        quote! {
            fn #dispatch(_: &mut App, _: ObjectId, _: u16, _: &[u8], _: &mut VecDeque<OwnedFd>) {}
        }
    } else {
        gen_events(iface, &tname, &events, &dispatch)
    };

    let methods: Vec<TokenStream> = requests
        .iter()
        .enumerate()
        .map(|(opcode, req)| gen_request(&iface.name, opcode as u16, req))
        .collect();

    quote! {
        #idoc
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub struct #tname(pub ObjectId);

        impl Interface for #tname {
            const NAME: &'static str = #name;
            const VERSION: u32 = #version;
            const INFO: &'static Info = &Info { name: #name, version: #version, dispatch: #dispatch };
            fn id(self) -> ObjectId { self.0 }
            fn from_id(id: ObjectId) -> Self { Self(id) }
        }

        impl Resource for #tname {}

        impl #tname {
            #(#methods)*
        }

        #event_items
        #(#enums)*
    }
}

// ── enums ────────────────────────────────────────────────────────────────────

fn gen_enum(iface: &str, en: &EnumDef) -> TokenStream {
    let ename = enum_ident(iface, &en.name);
    let edoc = doc(en.description.as_ref());
    let mut seen = HashSet::new();
    let entries: Vec<&EnumEntry> = en
        .entries
        .iter()
        .filter(|e| seen.insert(e.value.clone()))
        .collect();

    if en.bitfield {
        let consts: Vec<TokenStream> = entries
            .iter()
            .map(|e| {
                let v = id(&variant_name(&e.name).to_uppercase());
                let val: TokenStream = e.value.parse().unwrap();
                let s = summary(e.summary.as_deref());
                quote! { #s const #v = #val; }
            })
            .collect();
        quote! {
            bitflags! {
                #edoc
                #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
                pub struct #ename: u32 { #(#consts)* }
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
        let variants: Vec<TokenStream> = entries
            .iter()
            .map(|e| {
                let v = id(&variant_name(&e.name));
                let s = summary(e.summary.as_deref());
                quote! { #s #v, }
            })
            .collect();
        let from_arms: Vec<TokenStream> = entries
            .iter()
            .map(|e| {
                let v = id(&variant_name(&e.name));
                let val: TokenStream = e.value.parse().unwrap();
                quote! { #val => Ok(Self::#v), }
            })
            .collect();
        let into_arms: Vec<TokenStream> = entries
            .iter()
            .map(|e| {
                let v = id(&variant_name(&e.name));
                let val: TokenStream = e.value.parse().unwrap();
                quote! { #ename::#v => #val, }
            })
            .collect();
        quote! {
            #edoc
            #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
            pub enum #ename { #(#variants)* }
            impl TryFrom<u32> for #ename {
                type Error = u32;
                fn try_from(v: u32) -> Result<Self, u32> {
                    match v { #(#from_arms)* other => Err(other) }
                }
            }
            impl From<#ename> for u32 {
                fn from(v: #ename) -> u32 { match v { #(#into_arms)* } }
            }
        }
    }
}

// ── events ───────────────────────────────────────────────────────────────────

fn gen_events(
    iface: &Interface,
    tname: &Ident,
    events: &[&Message],
    dispatch: &Ident,
) -> TokenStream {
    let ename = id(&format!("{tname}Event"));
    let sender = sender_name(&iface.name);
    for ev in events {
        if ev.args.iter().any(|a| a.name == sender) {
            panic!(
                "{}.{}: an argument is named like the sender field `{sender}`",
                iface.name, ev.name
            );
        }
    }
    let sender = id(&sender);

    let variants: Vec<TokenStream> = events
        .iter()
        .map(|ev| {
            let v = id(&variant_name(&ev.name));
            let vdoc = doc(ev.description.as_ref());
            let fields: Vec<TokenStream> = ev
                .args
                .iter()
                .map(|a| {
                    let f = id(&a.name);
                    let t = event_field_type(&iface.name, a);
                    let s = summary(a.summary.as_deref());
                    quote! { #s #f: #t }
                })
                .collect();
            quote! { #vdoc #v { #sender: #tname, #(#fields),* }, }
        })
        .collect();

    let arms: Vec<TokenStream> = events
        .iter()
        .enumerate()
        .map(|(opcode, ev)| {
            let opcode = opcode as u16;
            let v = id(&variant_name(&ev.name));
            // Every `fd` argument is popped first, in wire order, before
            // anything that can fail to parse: a `?` on a later argument
            // then drops (closes) this message's fds rather than leaving
            // them in the queue for the next event to pop by mistake.
            // `fds` is positional, so one mis-consumed fd desyncs the rest
            // of the connection. Popping early is safe because an `fd`
            // argument takes no bytes off the wire, so it does not move
            // the reader.
            let (fd_args, body_args): (Vec<&Arg>, Vec<&Arg>) =
                ev.args.iter().partition(|a| a.arg_type == ArgType::Fd);
            let reads: Vec<TokenStream> = fd_args
                .iter()
                .chain(body_args.iter())
                .map(|a| gen_read(&iface.name, a))
                .collect();
            let names: Vec<Ident> = ev.args.iter().map(|a| id(&a.name)).collect();
            quote! {
                #opcode => {
                    #(#reads)*
                    Some(#ename::#v { #sender, #(#names),* })
                }
            }
        })
        .collect();

    quote! {
        #[derive(Debug)]
        pub enum #ename { #(#variants)* }
        impl Signal for #ename {}

        impl #ename {
            /// One event of this interface from its body, or `None` if the
            /// opcode is unknown or the body is short.
            pub fn decode(app: &mut App, sender: ObjectId, opcode: u16, body: &[u8], fds: &mut VecDeque<OwnedFd>) -> Option<Self> {
                let #sender = #tname(sender);
                let mut r = Reader::new(body);
                let _ = (&mut r, &*fds, &*app);
                match opcode {
                    #(#arms)*
                    _ => None,
                }
            }
        }

        fn #dispatch(app: &mut App, sender: ObjectId, opcode: u16, body: &[u8], fds: &mut VecDeque<OwnedFd>) {
            if let Some(event) = #ename::decode(app, sender, opcode, body, fds) {
                app.signal(event);
            }
        }
    }
}

fn event_field_type(iface: &str, a: &Arg) -> TokenStream {
    match a.arg_type {
        ArgType::Int | ArgType::Uint => match &a.enum_type {
            Some(e) => {
                let t = enum_ident(iface, e);
                quote! { #t }
            }
            None if a.arg_type == ArgType::Int => quote! { i32 },
            None => quote! { u32 },
        },
        ArgType::Fixed => quote! { f32 },
        ArgType::String => {
            if a.allow_null {
                quote! { Option<String> }
            } else {
                quote! { String }
            }
        }
        ArgType::Array => quote! { Vec<u8> },
        ArgType::Fd => quote! { OwnedFd },
        ArgType::NewId => match &a.interface {
            Some(i) => {
                let t = type_ident(i);
                quote! { #t }
            }
            None => quote! { ObjectId },
        },
        ArgType::Object => match (&a.interface, a.allow_null) {
            (Some(i), true) => {
                let t = type_ident(i);
                quote! { Option<#t> }
            }
            (Some(i), false) => {
                let t = type_ident(i);
                quote! { #t }
            }
            (None, true) => quote! { Option<ObjectId> },
            (None, false) => quote! { ObjectId },
        },
    }
}

fn gen_read(iface: &str, a: &Arg) -> TokenStream {
    let f = id(&a.name);
    match a.arg_type {
        ArgType::Int | ArgType::Uint => match &a.enum_type {
            Some(e) => {
                let t = enum_ident(iface, e);
                quote! { let #f = #t::try_from(r.uint()?).ok()?; }
            }
            None if a.arg_type == ArgType::Int => quote! { let #f = r.int()?; },
            None => quote! { let #f = r.uint()?; },
        },
        ArgType::Fixed => quote! { let #f = r.fixed()?; },
        ArgType::String => {
            if a.allow_null {
                quote! { let #f = r.string_opt()?; }
            } else {
                quote! { let #f = r.string()?; }
            }
        }
        ArgType::Array => quote! { let #f = r.array()?; },
        ArgType::Fd => quote! { let #f = fds.pop_front()?; },
        ArgType::NewId => match &a.interface {
            Some(i) => {
                let t = type_ident(i);
                let me = type_ident(iface);
                quote! {
                    let #f = #t(r.object()?);
                    let version = app.resource::<Wayland>().version(#me(sender));
                    app.resource_mut::<Wayland>().register(#f.0, #t::INFO, version);
                }
            }
            None => {
                quote! { let #f = { let id = r.object()?; let _ = r.string()?; let _ = r.uint()?; id }; }
            }
        },
        ArgType::Object => match (&a.interface, a.allow_null) {
            (Some(i), true) => {
                let t = type_ident(i);
                quote! { let #f = r.object_opt()?.map(#t); }
            }
            (Some(i), false) => {
                let t = type_ident(i);
                quote! { let #f = #t(r.object()?); }
            }
            (None, true) => quote! { let #f = r.object_opt()?; },
            (None, false) => quote! { let #f = r.object()?; },
        },
    }
}

// ── requests ─────────────────────────────────────────────────────────────────

fn gen_request(iface: &str, opcode: u16, req: &Message) -> TokenStream {
    let mname = id(&req.name);
    let rdoc = doc(req.description.as_ref());
    let new_id = req.args.iter().find(|a| a.arg_type == ArgType::NewId);
    if let Some(a) = new_id
        && a.interface.is_none()
    {
        panic!(
            "{iface}.{}: a new_id without an interface is only wl_registry.bind, which is hand-written",
            req.name
        );
    }

    let params: Vec<TokenStream> = req
        .args
        .iter()
        .filter(|a| a.arg_type != ArgType::NewId)
        .map(|a| {
            let p = id(&a.name);
            let t = request_param_type(iface, a);
            quote! { #p: #t }
        })
        .collect();

    let alloc = new_id.map(|a| {
        let f = id(&a.name);
        let t = type_ident(a.interface.as_ref().unwrap());
        quote! {
            let version = wl.version(self);
            let #f: #t = wl.alloc(version);
        }
    });
    let writes: Vec<TokenStream> = req.args.iter().map(gen_write).collect();
    let ret_type = new_id.map(|a| {
        let t = type_ident(a.interface.as_ref().unwrap());
        quote! { -> #t }
    });
    let ret = new_id.map(|a| {
        let f = id(&a.name);
        quote! { #f }
    });

    quote! {
        #rdoc
        pub fn #mname(self, wl: &mut Wayland, #(#params),*) #ret_type {
            #alloc
            wl.request(self.0, #opcode, |w| { let _ = &w; #(#writes)* });
            #ret
        }
    }
}

fn request_param_type(iface: &str, a: &Arg) -> TokenStream {
    match a.arg_type {
        ArgType::Int | ArgType::Uint => match &a.enum_type {
            Some(e) => {
                let t = enum_ident(iface, e);
                quote! { #t }
            }
            None if a.arg_type == ArgType::Int => quote! { i32 },
            None => quote! { u32 },
        },
        ArgType::Fixed => quote! { f32 },
        ArgType::String => {
            if a.allow_null {
                quote! { Option<&str> }
            } else {
                quote! { &str }
            }
        }
        ArgType::Array => quote! { &[u8] },
        ArgType::Fd => quote! { BorrowedFd<'_> },
        ArgType::NewId => unreachable!("filtered"),
        ArgType::Object => match (&a.interface, a.allow_null) {
            (Some(i), true) => {
                let t = type_ident(i);
                quote! { Option<#t> }
            }
            (Some(i), false) => {
                let t = type_ident(i);
                quote! { #t }
            }
            (None, true) => quote! { Option<ObjectId> },
            (None, false) => quote! { ObjectId },
        },
    }
}

fn gen_write(a: &Arg) -> TokenStream {
    let f = id(&a.name);
    match a.arg_type {
        ArgType::Int => match &a.enum_type {
            Some(_) => quote! { w.int(u32::from(#f) as i32); },
            None => quote! { w.int(#f); },
        },
        ArgType::Uint => match &a.enum_type {
            Some(_) => quote! { w.uint(u32::from(#f)); },
            None => quote! { w.uint(#f); },
        },
        ArgType::Fixed => quote! { w.fixed(#f); },
        ArgType::String => {
            if a.allow_null {
                quote! { w.string_opt(#f); }
            } else {
                quote! { w.string(#f); }
            }
        }
        ArgType::Array => quote! { w.array(#f); },
        ArgType::Fd => quote! { w.fd(#f); },
        ArgType::NewId => quote! { w.new_id(#f.0); },
        ArgType::Object => match (&a.interface, a.allow_null) {
            (Some(_), true) => quote! { w.object_opt(#f.map(|o| o.0)); },
            (Some(_), false) => quote! { w.object(#f.0); },
            (None, true) => quote! { w.object_opt(#f); },
            (None, false) => quote! { w.object(#f); },
        },
    }
}
