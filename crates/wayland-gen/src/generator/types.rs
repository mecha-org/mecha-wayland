use crate::ast::{Arg, ArgType, Protocol};
use crate::generator::utils::{safe_ident, to_pascal_case};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

/// Resolves the strict Rust type for a struct field
pub fn map_field_type(arg: &Arg, current_iface: &str) -> TokenStream {
    let base_type = resolve_base_type(arg, current_iface);

    if arg.allow_null {
        quote!(Option<#base_type>)
    } else {
        base_type
    }
}

/// Generates the reader chain for `WaylandParse`
pub fn map_reader(arg: &Arg, protocol: &Protocol, current_iface: &str) -> TokenStream {
    let reader = match arg.arg_type {
        ArgType::Int => quote!(r.read_i32()?),
        ArgType::Uint | ArgType::NewId | ArgType::Object => quote!(r.read_u32()?),
        ArgType::Fixed => quote!(r.read_fixed()?),
        ArgType::String => quote!(r.read_string()?.to_string()),
        ArgType::Array => quote!(r.read_array()?.to_vec()),
        ArgType::Fd => quote!(r.read_fd()?),
    };

    if let Some(enum_name) = &arg.enum_type {
        let is_bitfield = protocol
            .find_enum(current_iface, enum_name)
            .is_some_and(|e| e.bitfield);
        let enum_type = map_field_type(arg, current_iface);

        if is_bitfield {
            quote!( #enum_type::from_bits_retain((#reader) as u32) )
        } else if arg.allow_null {
            // Nullable enum: 0 is None
            quote!( { let v = #reader; if v == 0 { None } else { Some(#enum_type::from_wire(v as u32)) } } )
        } else {
            // Infallible enum parsing using Unknown(u32)
            quote!( #enum_type::from_wire((#reader) as u32) )
        }
    } else if arg.allow_null {
        match arg.arg_type {
            ArgType::Object | ArgType::NewId | ArgType::Uint | ArgType::Int => {
                quote!( { let v = #reader; if v == 0 { None } else { Some(v) } } )
            }
            ArgType::String => {
                quote!( { let s = #reader; if s.is_empty() { None } else { Some(s) } } )
            }
            _ => quote!( Some(#reader) ),
        }
    } else {
        reader
    }
}

/// Generates the builder chain for `WaylandSend`
pub fn map_writer(arg: &Arg, protocol: &Protocol, current_iface: &str) -> TokenStream {
    let arg_name = safe_ident(&arg.name);

    let accessor = if let Some(enum_name) = &arg.enum_type {
        let is_bitfield = protocol
            .find_enum(current_iface, enum_name)
            .is_some_and(|e| e.bitfield);

        match (arg.allow_null, is_bitfield) {
            (true, true) => quote!( self.#arg_name.map_or(0, |e| e.bits()) ),
            (true, false) => quote!( self.#arg_name.map_or(0, |e| e.to_wire()) ),
            (false, true) => quote!( self.#arg_name.bits() ),
            (false, false) => quote!( self.#arg_name.to_wire() ),
        }
    } else if arg.allow_null {
        match arg.arg_type {
            ArgType::String => quote!( self.#arg_name.as_deref().unwrap_or("") ),
            ArgType::Array => quote!( self.#arg_name.as_deref().unwrap_or(&[]) ),
            _ => quote!( self.#arg_name.unwrap_or(0) ),
        }
    } else {
        match arg.arg_type {
            ArgType::String | ArgType::Array => quote!( &self.#arg_name ),
            _ => quote!( self.#arg_name ),
        }
    };

    match arg.arg_type {
        ArgType::String => quote!( .write_string(#accessor) ),
        ArgType::Object | ArgType::NewId | ArgType::Uint => {
            quote!( .write_u32((#accessor) as u32) )
        }
        ArgType::Int => quote!( .write_i32((#accessor) as i32) ),
        ArgType::Fixed => quote!( .write_fixed(#accessor) ),
        ArgType::Array => quote!( .write_array(#accessor) ),
        ArgType::Fd => quote!( .write_fd(#accessor) ),
    }
}

fn resolve_base_type(arg: &Arg, current_iface: &str) -> TokenStream {
    if let Some(enum_name) = &arg.enum_type {
        let (iface, e_name) = enum_name
            .split_once('.')
            .unwrap_or((current_iface, enum_name.as_str()));

        let iface_mod = format_ident!("{}", iface);
        let ident = format_ident!("{}{}", to_pascal_case(iface), to_pascal_case(e_name));

        quote!(crate::proto::#iface_mod::#ident)
    } else {
        match arg.arg_type {
            ArgType::Int => quote!(i32),
            ArgType::Uint | ArgType::Object | ArgType::NewId => quote!(u32),
            ArgType::Fixed => quote!(f64),
            ArgType::String => quote!(String),
            ArgType::Array => quote!(Vec<u8>),
            ArgType::Fd => quote!(std::os::unix::io::RawFd),
        }
    }
}
