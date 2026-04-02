#![allow(dead_code)]

use std::env;
use std::fs;
use std::io::Write;

use quick_xml::de::from_str;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Protocol {
    #[serde(rename = "@name")]
    name: String,
    #[serde(rename = "$value")]
    items: Vec<ProtocolItem>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum ProtocolItem {
    Copyright(Copyright),
    Interface(Interface),
    Description(Description),
}

#[derive(Debug, Deserialize)]
struct Copyright {
    #[serde(rename = "$value")]
    text: String,
}

#[derive(Debug, Deserialize)]
struct Interface {
    #[serde(rename = "@name")]
    name: String,
    #[serde(rename = "@version")]
    version: u32,
    #[serde(rename = "$value")]
    items: Vec<InterfaceItem>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum InterfaceItem {
    Description(Description),
    Request(Request),
    Event(Event),
    Enum(Enum),
}

#[derive(Debug, Deserialize)]
struct Request {
    #[serde(rename = "@name")]
    name: String,
    #[serde(rename = "@type")]
    req_type: Option<String>,
    #[serde(rename = "@since")]
    since: Option<u32>,
    #[serde(rename = "$value")]
    items: Vec<RequestItem>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum RequestItem {
    Description(Description),
    Arg(Arg),
}

#[derive(Debug, Deserialize)]
struct Event {
    #[serde(rename = "@name")]
    name: String,
    #[serde(rename = "@since")]
    since: Option<u32>,
    #[serde(rename = "$value")]
    items: Vec<EventItem>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum EventItem {
    Description(Description),
    Arg(Arg),
}

#[derive(Debug, Deserialize)]
struct Enum {
    #[serde(rename = "@name")]
    name: String,
    #[serde(rename = "@since")]
    since: Option<u32>,
    #[serde(rename = "@bitfield")]
    bitfield: Option<bool>,
    #[serde(rename = "$value")]
    items: Vec<EnumItem>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum EnumItem {
    Description(Description),
    Entry(Entry),
}

#[derive(Debug, Deserialize)]
struct Entry {
    #[serde(rename = "@name")]
    name: String,
    #[serde(rename = "@value")]
    value: String,
    #[serde(rename = "@summary")]
    summary: Option<String>,
    #[serde(rename = "@since")]
    since: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct Arg {
    #[serde(rename = "@name")]
    name: String,
    #[serde(rename = "@type")]
    arg_type: String,
    #[serde(rename = "@summary")]
    summary: Option<String>,
    #[serde(rename = "@interface")]
    interface: Option<String>,
    #[serde(rename = "@allow-null")]
    allow_null: Option<bool>,
    #[serde(rename = "@enum")]
    enum_type: Option<String>,
    #[serde(rename = "description")]
    description: Option<Description>,
}

#[derive(Debug, Deserialize)]
struct Description {
    #[serde(rename = "@summary")]
    summary: Option<String>,
    #[serde(rename = "$value")]
    text: Option<String>,
}

// ── Child-accessor methods ─────────────────────────────────────────────────────

impl Protocol {
    fn interfaces(&self) -> Vec<&Interface> {
        self.items
            .iter()
            .filter_map(|item| match item {
                ProtocolItem::Interface(i) => Some(i),
                _ => None,
            })
            .collect()
    }
}

impl Interface {
    fn requests(&self) -> Vec<&Request> {
        self.items
            .iter()
            .filter_map(|item| match item {
                InterfaceItem::Request(r) => Some(r),
                _ => None,
            })
            .collect()
    }

    fn events(&self) -> Vec<&Event> {
        self.items
            .iter()
            .filter_map(|item| match item {
                InterfaceItem::Event(e) => Some(e),
                _ => None,
            })
            .collect()
    }
}

impl Request {
    fn args(&self) -> Vec<&Arg> {
        self.items
            .iter()
            .filter_map(|item| match item {
                RequestItem::Arg(a) => Some(a),
                _ => None,
            })
            .collect()
    }
}

impl Event {
    fn args(&self) -> Vec<&Arg> {
        self.items
            .iter()
            .filter_map(|item| match item {
                EventItem::Arg(a) => Some(a),
                _ => None,
            })
            .collect()
    }
}

impl Arg {
    fn rust_enum_name(&self, current_iface: &str) -> Option<String> {
        self.enum_type.as_ref().map(|e| {
            if e.contains('.') {
                // Global enum (e.g., "wl_output.transform" -> "WlOutputTransform")
                to_pascal_case(&e.replace('.', "_"))
            } else {
                // Local enum (e.g., "anchor" -> "ZwlrLayerSurfaceV1Anchor")
                to_pascal_case(&format!("{}_{}", current_iface, e))
            }
        })
    }

    /// Rust type for an event struct field.
    fn rust_type(&self, current_iface: &str) -> String {
        // If it is an enum, return the enum name
        if let Some(enum_name) = self.rust_enum_name(current_iface) {
            return enum_name;
        }

        // Otherwise, fall back to standard types
        match self.arg_type.as_str() {
            "int" | "fixed" => "i32".to_string(),
            "uint" | "object" | "new_id" => "u32".to_string(),
            "string" => "String".to_string(),
            "array" => "Vec<u8>".to_string(),
            "fd" => "std::os::unix::io::OwnedFd".to_string(),
            _ => "u32".to_string(),
        }
    }

    /// Rust type for a request method parameter.
    fn param_type(&self, current_iface: &str) -> String {
        if let Some(enum_name) = self.rust_enum_name(current_iface) {
            return enum_name;
        }

        match self.arg_type.as_str() {
            "int" | "fixed" => "i32".to_string(),
            "uint" => "u32".to_string(),
            "object" | "new_id" => "impl crate::object::Object".to_string(),
            "string" => "&str".to_string(),
            "array" => "&[u8]".to_string(),
            "fd" => "std::os::unix::io::OwnedFd".to_string(),
            _ => "u32".to_string(),
        }
    }

    /// Encode expression for a named parameter.
    fn encode(&self, param: &str, current_iface: &str) -> String {
        if self.rust_enum_name(current_iface).is_some() {
            return format!("crate::wire::write_u32(&mut args, {}.bits());", param);
        }

        match self.arg_type.as_str() {
            "uint" => format!("crate::wire::write_u32(&mut args, {});", param),
            "object" | "new_id" => {
                format!("crate::wire::write_u32(&mut args, {}.object_id());", param)
            }
            "int" | "fixed" => {
                format!("crate::wire::write_u32(&mut args, {} as u32);", param)
            }
            "string" => format!("crate::wire::write_string(&mut args, {});", param),
            "array" => format!(
                "{{ let _arr = {}; crate::wire::write_u32(&mut args, _arr.len() as u32); \
                 args.extend_from_slice(_arr); \
                 let _pad = (4 - (_arr.len() % 4)) % 4; for _ in 0.._pad {{ args.push(0); }} }}",
                param
            ),
            "fd" => format!("fds.push({});", param),
            _ => format!("crate::wire::write_u32(&mut args, {});", param),
        }
    }
}

// ── Name helpers ───────────────────────────────────────────────────────────────

fn to_const_name(s: &str) -> String {
    s.to_ascii_uppercase().replace('-', "_")
}

/// Prefix Rust keywords with `r#` so they can be used as identifiers.
fn escape_keyword(s: &str) -> String {
    match s {
        "as" | "async" | "await" | "break" | "const" | "continue" | "crate" | "dyn" | "else"
        | "enum" | "extern" | "false" | "fn" | "for" | "if" | "impl" | "in" | "let" | "loop"
        | "match" | "mod" | "move" | "mut" | "pub" | "ref" | "return" | "self" | "Self"
        | "static" | "struct" | "super" | "trait" | "true" | "type" | "union" | "unsafe"
        | "use" | "where" | "while" => format!("r#{}", s),
        _ => s.to_string(),
    }
}

fn to_pascal_case(s: &str) -> String {
    s.split('_')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().to_string() + c.as_str(),
            }
        })
        .collect()
}

fn to_valid_enum_variant(s: &str) -> String {
    let pascal = to_pascal_case(s);

    // if the string starts with a digit, prefix with 'D'.
    if pascal.starts_with(|c: char| c.is_ascii_digit()) {
        format!("D{}", pascal)
    } else {
        pascal
    }
}

// ── Named return types ─────────────────────────────────────────────────────────

struct RequestBody {
    extra_params: String,
    body_stmts: String,
    send_call: String,
    has_fds: bool,
}

struct DecodeResult {
    stmts: String,
    field_names: Vec<String>,
}

// ── Offset state machine ───────────────────────────────────────────────────────

struct OffsetTracker {
    fixed: usize,
    dynamic: Option<String>,
}

impl OffsetTracker {
    fn new() -> Self {
        OffsetTracker {
            fixed: 0,
            dynamic: None,
        }
    }

    fn current(&self) -> String {
        match &self.dynamic {
            None => self.fixed.to_string(),
            Some(v) => v.clone(),
        }
    }

    /// Advance by 4 bytes for a fixed-size field at position `i`.
    /// Returns an offset binding statement to emit when dynamic tracking is
    /// active; `None` when the offset is still a compile-time literal.
    fn advance_fixed(&mut self, i: usize) -> Option<String> {
        if self.dynamic.is_none() {
            self.fixed += 4;
            None
        } else {
            let nv = format!("_off{}", i);
            let stmt = format!(
                "                let {} = {} + 4;\n",
                nv,
                self.dynamic.as_ref().unwrap()
            );
            self.dynamic = Some(nv);
            Some(stmt)
        }
    }

    fn set_dynamic(&mut self, var: String) {
        self.dynamic = Some(var);
    }
}

// ── Data builders ─────────────────────────────────────────────────────────────

/// Builds the variable parts of a request method body.
fn build_request_body(iface_name: &str, req: &Request) -> RequestBody {
    let args = req.args();
    let has_fds = args.iter().any(|a| a.arg_type == "fd");
    let opcode_path = format!(
        "{}_proto::request::{}",
        iface_name,
        to_const_name(&req.name)
    );

    let mut params = String::new();
    let mut encode_lines: Vec<String> = Vec::new();

    for arg in &args {
        if arg.arg_type == "new_id" && arg.interface.is_none() {
            if !params.is_empty() {
                params.push_str(", ");
            }
            params.push_str("interface: &str, version: u32, id: impl crate::object::Object");
            encode_lines.push("crate::wire::write_string(&mut args, interface);".to_string());
            encode_lines.push("crate::wire::write_u32(&mut args, version);".to_string());
            encode_lines.push("crate::wire::write_u32(&mut args, id.object_id());".to_string());
        } else {
            let pname = escape_keyword(&arg.name);
            if !params.is_empty() {
                params.push_str(", ");
            }
            params.push_str(&format!("{}: {}", pname, arg.param_type(iface_name)));
            encode_lines.push(arg.encode(&pname, iface_name));
        }
    }

    let extra_params = if params.is_empty() {
        String::new()
    } else {
        format!(", {}", params)
    };

    let body_stmts = if encode_lines.is_empty() {
        "        let args: &[u8] = &[];\n".to_string()
    } else {
        let mut s = "        let mut args = Vec::new();\n".to_string();
        if has_fds {
            s.push_str("        let mut fds: Vec<std::os::unix::io::OwnedFd> = Vec::new();\n");
        }
        for line in &encode_lines {
            s.push_str(&format!("        {}\n", line));
        }
        s
    };

    let send_call = if has_fds {
        format!(
            "conn.send_msg_with_fds(self.object_id(), {}, &args, fds)",
            opcode_path
        )
    } else {
        format!("conn.send_msg(self.object_id(), {}, &args)", opcode_path)
    };

    RequestBody {
        extra_params,
        body_stmts,
        send_call,
        has_fds,
    }
}

/// Decodes event args into variable-binding statements.
/// Each decode stmt has 16-space indent.
fn build_decode_stmts(iface_name: &str, args: &[&Arg]) -> DecodeResult {
    let mut tracker = OffsetTracker::new();
    let mut field_names: Vec<String> = Vec::new();
    let mut stmts = String::new();

    for (i, arg) in args.iter().enumerate() {
        let var = escape_keyword(&arg.name);
        field_names.push(var.clone());

        if arg.arg_type == "fd" {
            stmts.push_str(&format!("                let {} = conn.pop_fd()?;\n", var));
            continue;
        }

        let cur_off = tracker.current();

        if let Some(enum_name) = arg.rust_enum_name(iface_name) {
            stmts.push_str(&format!(
                "                let {}_val = crate::wire::read_u32(body, {});\n",
                var, cur_off
            ));
            stmts.push_str(&format!(
                "                let {} = match {}::from_bits({}_val) {{\n",
                var, enum_name, var
            ));
            stmts.push_str(&format!("                    Some(v) => v,\n"));
            stmts.push_str(&format!(
                "                    None => return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, \"Unknown enum variant\")),\n"
            ));
            stmts.push_str(&format!("                }};\n"));

            if let Some(off_stmt) = tracker.advance_fixed(i) {
                stmts.push_str(&off_stmt);
            }
            continue; // Skip the standard match below
        }

        match arg.arg_type.as_str() {
            "uint" | "object" | "new_id" => {
                stmts.push_str(&format!(
                    "                let {} = crate::wire::read_u32(body, {});\n",
                    var, cur_off
                ));
                if let Some(off_stmt) = tracker.advance_fixed(i) {
                    stmts.push_str(&off_stmt);
                }
            }
            "int" | "fixed" => {
                stmts.push_str(&format!(
                    "                let {} = crate::wire::read_u32(body, {}) as i32;\n",
                    var, cur_off
                ));
                if let Some(off_stmt) = tracker.advance_fixed(i) {
                    stmts.push_str(&off_stmt);
                }
            }
            "string" => {
                let nv = format!("_off{}", i);
                stmts.push_str(&format!(
                    "                let ({}, {}) = crate::wire::read_string(body, {});\n",
                    var, nv, cur_off
                ));
                tracker.set_dynamic(nv);
            }
            "array" => {
                let len_var = format!("_arr_len{}", i);
                let nv = format!("_off{}", i);
                stmts.push_str(&format!(
                    "                let {} = crate::wire::read_u32(body, {}) as usize;\n",
                    len_var, cur_off
                ));
                stmts.push_str(&format!(
                    "                let {} = body[{} + 4..{} + 4 + {}].to_vec();\n",
                    var, cur_off, cur_off, len_var
                ));
                stmts.push_str(&format!(
                    "                let {} = {} + 4 + {} + (4 - ({} % 4)) % 4;\n",
                    nv, cur_off, len_var, len_var
                ));
                tracker.set_dynamic(nv);
            }
            _ => {
                stmts.push_str(&format!(
                    "                let {} = crate::wire::read_u32(body, {});\n",
                    var, cur_off
                ));
                if let Some(off_stmt) = tracker.advance_fixed(i) {
                    stmts.push_str(&off_stmt);
                }
            }
        }
    }

    DecodeResult { stmts, field_names }
}

// ── Snippet functions ─────────────────────────────────────────────────────────

/// Pass-1 constant module for one interface.
fn snippet_const_module(
    iface_name: &str,
    version: u32,
    req_consts: &str,
    evt_consts: &str,
) -> String {
    let req_block = if req_consts.is_empty() {
        String::new()
    } else {
        format!("    pub mod request {{\n{req_consts}    }}\n")
    };
    let evt_block = if evt_consts.is_empty() {
        String::new()
    } else {
        format!("    pub mod event {{\n{evt_consts}    }}\n")
    };
    let mod_name = format!("{iface_name}_proto");
    format!(
        "pub mod {mod_name} {{\n    pub const INTERFACE: &str = {iface_name:?};\n    pub const VERSION: u32 = {version};\n{req_block}{evt_block}}}\n\n"
    )
}

/// Event struct with one or more fields.
fn snippet_event_struct(struct_name: &str, fields: &str) -> String {
    format!(
        r#"pub struct {struct_name} {{
{fields}}}

"#
    )
}

/// Concrete object struct + Object impl.
fn snippet_concrete_type(pascal: &str) -> String {
    format!(
        r#"pub struct {pascal} {{
    object_id: u32,
}}

impl {pascal} {{
    pub fn new(object_id: u32) -> Self {{
        {pascal} {{ object_id }}
    }}
}}

impl crate::object::Object for {pascal} {{
    fn object_id(&self) -> u32 {{ self.object_id }}
}}

"#
    )
}

/// Inherent impl block containing public request methods.
fn snippet_inherent_impl(pascal: &str, methods: &str) -> String {
    if methods.is_empty() {
        return String::new();
    }
    format!("impl {pascal} {{\n{methods}}}\n\n")
}

/// One request method as a `pub` inherent method or a non-pub trait method.
/// `body_stmts` already has 8-space indentation and trailing newline(s).
fn snippet_request_method(
    method_name: &str,
    extra_params: &str,
    log_path: &str,
    iface_name: &str,
    req_name: &str,
    body_stmts: &str,
    send_call: &str,
    is_pub: bool,
) -> String {
    let pub_kw = if is_pub { "pub " } else { "" };
    format!(
        r#"    {pub_kw}fn {method_name}(&self, conn: &mut crate::connection::Connection{extra_params}) -> std::io::Result<()> {{
        tracing::debug!(object_id = self.object_id(), opcode = {log_path}, "{iface_name}.{req_name}");
{body_stmts}        {send_call}
    }}

"#
    )
}

/// Default impl of a handler trait for the generated concrete type.
fn snippet_default_impl(trait_name: &str, pascal: &str) -> String {
    format!("impl {trait_name} for {pascal} {{}}\n\n")
}

/// One match arm inside dispatch.
/// `decode_stmts` already has 16-space indentation and trailing newline (or is empty).
fn snippet_dispatch_arm(
    opcode_const: &str,
    iface_name: &str,
    evt_name: &str,
    decode_stmts: &str,
    call: &str,
) -> String {
    format!(
        r#"            {opcode_const} => {{
                tracing::trace!(opcode = {opcode_const}, "{iface_name}.{evt_name}");
{decode_stmts}                {call};
            }}
"#
    )
}

/// Full dispatch method for an interface that has at least one event.
/// `match_arms` is the concatenation of `snippet_dispatch_arm` results.
fn snippet_dispatch(conn_param: &str, body_param: &str, match_arms: &str) -> String {
    format!(
        r#"    fn dispatch(&mut self, {conn_param}: &mut crate::connection::Connection, opcode: u16, {body_param}: &[u8]) -> std::io::Result<()> {{
        match opcode {{
{match_arms}            _ => {{}}
        }}
        Ok(())
    }}
"#
    )
}

/// Dispatch method for interfaces with no events.
fn snippet_dispatch_empty() -> String {
    "    fn dispatch(&mut self, _conn: &mut crate::connection::Connection, opcode: u16, _body: &[u8]) -> std::io::Result<()> {\n        let _ = opcode;\n        Ok(())\n    }\n".to_string()
}

/// Complete handler trait (event callbacks + provided dispatch only; no request methods).
/// `callbacks` — concatenated `fn on_*(...) {}` defaults (each ends with `\n`), or `""`.
/// `dispatch`  — the dispatch snippet (ends with `\n`).
fn snippet_handler_trait(trait_name: &str, callbacks: &str, dispatch: &str) -> String {
    let sep = if callbacks.is_empty() { "" } else { "\n" };
    format!("pub trait {trait_name}: crate::object::Object {{\n{callbacks}{sep}{dispatch}}}\n\n")
}

// ── Protocol generator ────────────────────────────────────────────────────────

fn emit_const_modules(f: &mut impl Write, interfaces: &[&Interface]) -> anyhow::Result<()> {
    for iface in interfaces {
        let req_consts: String = iface
            .requests()
            .iter()
            .enumerate()
            .map(|(op, req)| {
                format!(
                    "        pub const {}: u16 = {};\n",
                    to_const_name(&req.name),
                    op
                )
            })
            .collect();

        let evt_consts: String = iface
            .events()
            .iter()
            .enumerate()
            .map(|(op, evt)| {
                format!(
                    "        pub const {}: u16 = {};\n",
                    to_const_name(&evt.name),
                    op
                )
            })
            .collect();

        write!(
            f,
            "{}",
            snippet_const_module(&iface.name, iface.version, &req_consts, &evt_consts)
        )?;
    }
    Ok(())
}

fn emit_handler_traits(f: &mut impl Write, interfaces: &[&Interface]) -> anyhow::Result<()> {
    for iface in interfaces {
        let iface_pascal = to_pascal_case(&iface.name);
        let requests = iface.requests();
        let events = iface.events();

        // Event structs (only for events with ≥1 arg)
        for evt in &events {
            let args = evt.args();
            if args.is_empty() {
                continue;
            }
            let struct_name = format!("{}{}Event", iface_pascal, to_pascal_case(&evt.name));
            let fields: String = args
                .iter()
                .map(|a| {
                    format!(
                        "    pub {}: {},\n",
                        escape_keyword(&a.name),
                        a.rust_type(&iface.name)
                    )
                })
                .collect();
            write!(f, "{}", snippet_event_struct(&struct_name, &fields))?;
        }

        // Concrete struct + Object impl
        write!(f, "{}", snippet_concrete_type(&iface_pascal))?;

        // Inherent impl — public request sender methods
        let inherent_methods: String = requests
            .iter()
            .map(|req| {
                let method_name = escape_keyword(&req.name.replace('-', "_"));
                let log_path = format!(
                    "{}_proto::request::{}",
                    iface.name,
                    to_const_name(&req.name)
                );
                let req_name = req.name.replace('-', "_");
                let rb = build_request_body(&iface.name, req);
                snippet_request_method(
                    &method_name,
                    &rb.extra_params,
                    &log_path,
                    &iface.name,
                    &req_name,
                    &rb.body_stmts,
                    &rb.send_call,
                    true,
                )
            })
            .collect();
        write!(
            f,
            "{}",
            snippet_inherent_impl(&iface_pascal, &inherent_methods)
        )?;

        // Handler trait — event callbacks + provided dispatch only
        let trait_name = format!("{}Handler", iface_pascal);

        let callbacks: String = events
            .iter()
            .map(|evt| {
                let args = evt.args();
                let cb_name = format!("on_{}", evt.name.replace('-', "_"));
                if args.is_empty() {
                    format!("    fn {}(&mut self) {{}}\n", cb_name)
                } else {
                    let struct_name = format!("{}{}Event", iface_pascal, to_pascal_case(&evt.name));
                    format!(
                        "    fn {}(&mut self, _event: {}) {{}}\n",
                        cb_name, struct_name
                    )
                }
            })
            .collect();

        let dispatch = if events.is_empty() {
            snippet_dispatch_empty()
        } else {
            let any_body_args = events
                .iter()
                .any(|evt| evt.args().into_iter().any(|a| a.arg_type != "fd"));
            let any_fd_args = events
                .iter()
                .any(|evt| evt.args().into_iter().any(|a| a.arg_type == "fd"));
            let body_param = if any_body_args { "body" } else { "_body" };
            let conn_param = if any_fd_args { "conn" } else { "_conn" };

            let match_arms: String = events
                .iter()
                .map(|evt| {
                    let args = evt.args();
                    let opcode_const =
                        format!("{}_proto::event::{}", iface.name, to_const_name(&evt.name));
                    let evt_name = evt.name.replace('-', "_");
                    let cb_name = format!("on_{}", evt_name);

                    let (decode_stmts, field_names) = if args.is_empty() {
                        (String::new(), Vec::new())
                    } else {
                        let dr = build_decode_stmts(&iface.name, &args);
                        (dr.stmts, dr.field_names)
                    };

                    let call = if args.is_empty() {
                        format!("self.{}()", cb_name)
                    } else {
                        let struct_name =
                            format!("{}{}Event", iface_pascal, to_pascal_case(&evt.name));
                        let fields = field_names.join(", ");
                        format!("self.{}({} {{ {} }})", cb_name, struct_name, fields)
                    };

                    snippet_dispatch_arm(
                        &opcode_const,
                        &iface.name,
                        &evt_name,
                        &decode_stmts,
                        &call,
                    )
                })
                .collect();

            snippet_dispatch(conn_param, body_param, &match_arms)
        };

        write!(
            f,
            "{}",
            snippet_handler_trait(&trait_name, &callbacks, &dispatch)
        )?;

        // Default impl — same crate, no orphan issue
        write!(f, "{}", snippet_default_impl(&trait_name, &iface_pascal))?;
    }
    Ok(())
}

fn emit_enums(f: &mut impl Write, interfaces: &[&Interface]) -> anyhow::Result<()> {
    // Helper closure to parse both hex and decimal strings
    let parse_val = |s: &str| -> anyhow::Result<u32> {
        if let Some(hex) = s.strip_prefix("0x") {
            Ok(u32::from_str_radix(hex, 16)?)
        } else {
            Ok(s.parse::<u32>()?)
        }
    };

    for iface in interfaces {
        for item in &iface.items {
            if let InterfaceItem::Enum(en) = item {
                let enum_name = to_pascal_case(&format!("{}_{}", iface.name, en.name));
                let is_bitfield = en.bitfield.unwrap_or(false);

                if is_bitfield {
                    // Generate Bitflags
                    writeln!(f, "::bitflags::bitflags! {{")?;
                    writeln!(f, "    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]")?;
                    writeln!(f, "    pub struct {}: u32 {{", enum_name)?;

                    let mut has_zero = false;
                    for entry in &en.items {
                        if let EnumItem::Entry(e) = entry {
                            let entry_name = to_valid_enum_variant(&e.name);
                            let val = parse_val(&e.value)?;

                            if val == 0 {
                                has_zero = true;
                            }
                            writeln!(f, "        const {} = {};", entry_name, val)?;
                        }
                    }
                    if !has_zero {
                        writeln!(f, "        const None = 0;")?;
                    }
                    writeln!(f, "    }}")?;
                    writeln!(f, "}}\n")?;
                } else {
                    // Generate Standard Enum
                    writeln!(f, "#[repr(u32)]")?;
                    // Added `Hash` to match bitflags derive list
                    writeln!(f, "#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]")?;
                    writeln!(f, "pub enum {} {{", enum_name)?;
                    for entry in &en.items {
                        if let EnumItem::Entry(e) = entry {
                            let entry_name = to_valid_enum_variant(&e.name);
                            let val = parse_val(&e.value)?;
                            writeln!(f, "    {} = {},", entry_name, val)?;
                        }
                    }
                    writeln!(f, "}}")?;

                    // Inject helper methods so both bitflags and enums share the same API
                    writeln!(f, "impl {} {{", enum_name)?;
                    writeln!(f, "    pub fn bits(&self) -> u32 {{ *self as u32 }}")?;

                    // Added `from_bits` to match bitflags for easy decoding later
                    writeln!(f, "    pub fn from_bits(val: u32) -> Option<Self> {{")?;
                    writeln!(f, "        match val {{")?;
                    for entry in &en.items {
                        if let EnumItem::Entry(e) = entry {
                            let entry_name = to_valid_enum_variant(&e.name);
                            let val = parse_val(&e.value)?;
                            writeln!(f, "            {} => Some(Self::{}),", val, entry_name)?;
                        }
                    }
                    writeln!(f, "            _ => None,")?;
                    writeln!(f, "        }}")?;
                    writeln!(f, "    }}")?;

                    writeln!(f, "}}\n")?;
                }
            }
        }
    }
    Ok(())
}

fn generate_protocol(f: &mut impl Write, filename: &str) -> anyhow::Result<()> {
    println!("cargo:rerun-if-changed={}", filename);

    let xml_content = fs::read_to_string(filename)
        .map_err(|e| anyhow::anyhow!("Failed to read '{}': {}", filename, e))?;
    let protocol: Protocol = from_str(&xml_content)?;
    let interfaces = protocol.interfaces();

    emit_enums(f, &interfaces)?;
    emit_const_modules(f, &interfaces)?;
    emit_handler_traits(f, &interfaces)?;

    Ok(())
}

fn open(s: &str) -> std::io::Result<std::io::BufWriter<std::fs::File>> {
    let out_dir = env::var("OUT_DIR").unwrap_or_else(|_| ".".to_string());
    let path = std::path::Path::new(&out_dir).join(s);
    Ok(std::io::BufWriter::new(std::fs::File::create(path)?))
}

fn main() -> anyhow::Result<()> {
    let mut f = open("wayland_protocol.rs")?;
    writeln!(f, "// Auto-generated. DO NOT EDIT.")?;
    writeln!(f)?;
    // Bring Object::object_id() into scope for generated inherent impls.
    writeln!(f, "#[allow(unused_imports)]")?;
    writeln!(f, "use crate::object::Object as _;")?;
    writeln!(f)?;

    generate_protocol(&mut f, "protocols/wayland.xml")?;
    generate_protocol(&mut f, "protocols/xdg-shell.xml")?;
    generate_protocol(&mut f, "protocols/linux-dmabuf-unstable-v1.xml")?;
    generate_protocol(&mut f, "protocols/wlr-layer-shell-unstable-v1.xml")?;

    Ok(())
}
