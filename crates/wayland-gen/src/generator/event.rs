use crate::ast::{Interface, Message, Protocol};
use crate::generator::{types::*, utils::*};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

pub fn generate_events(interface: &Interface, protocol: &Protocol) -> TokenStream {
    let events: Vec<_> = interface.events().collect();

    if events.is_empty() {
        return quote!();
    }

    let event_structs = events
        .iter()
        .enumerate()
        .map(|(i, event)| generate_event(i as u16, event, interface, protocol));

    quote! {
        pub mod event {
            use super::*;
            #(#event_structs)*
        }
    }
}

fn generate_event(
    opcode: u16,
    event: &Message,
    interface: &Interface,
    protocol: &Protocol,
) -> TokenStream {
    let event_struct_name = format_ident!("{}", to_pascal_case(&event.name));

    let (fields, reads): (Vec<_>, Vec<_>) = event
        .args
        .iter()
        .map(|arg| {
            let name = safe_ident(&arg.name);
            let ty = map_field_type(arg, &interface.name);
            let read_expr = map_reader(arg, protocol, &interface.name);

            (quote!(pub #name: #ty), quote!(#name: #read_expr))
        })
        .unzip();

    let is_empty = fields.is_empty();

    let struct_decl = if is_empty {
        quote!(pub struct #event_struct_name;)
    } else {
        quote! {
            pub struct #event_struct_name {
                #(#fields),* }
        }
    };

    let deserialize_impl = if is_empty {
        quote! {
            fn deserialize(_body: &[u8]) -> Option<Self> {
                Some(#event_struct_name)
            }
        }
    } else {
        quote! {
            fn deserialize(body: &[u8]) -> Option<Self> {
                let mut fds = vec![];
                let mut r = MessageReader::new(body, &mut fds);
                Some(#event_struct_name {
                    #(#reads),* })
            }
        }
    };

    quote! {
        #[derive(Debug)]
        #struct_decl

        impl WaylandParse for #event_struct_name {
            const OPCODE: u16 = #opcode;
            #deserialize_impl
        }
    }
}
