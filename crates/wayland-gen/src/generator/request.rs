use crate::ast::{ArgType, Interface, Message, Protocol};
use crate::generator::{types::*, utils::*};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

pub fn generate_requests(interface: &Interface, protocol: &Protocol) -> TokenStream {
    let requests: Vec<_> = interface.requests().collect();

    if requests.is_empty() {
        return quote!();
    }

    let request_structs = requests
        .iter()
        .enumerate()
        .map(|(i, req)| generate_request(i as u16, req, interface, protocol));

    quote! {
        pub mod request {
            use super::*;
            #(#request_structs)*
        }
    }
}

fn generate_request(
    opcode: u16,
    request: &Message,
    interface: &Interface,
    protocol: &Protocol,
) -> TokenStream {
    let request_struct_name = format_ident!("{}", to_pascal_case(&request.name));
    let struct_name = format_ident!("{}", to_pascal_case(&interface.name));

    let mut fields = Vec::new();
    let mut writes = Vec::new();

    for arg in &request.args {
        if arg.arg_type == ArgType::NewId && arg.interface.is_none() {
            let name = safe_ident(&arg.name);

            fields.push(quote!(pub interface: String));
            fields.push(quote!(pub version: u32));
            fields.push(quote!(pub #name: u32));

            writes.push(quote!(.write_string(&self.interface)));
            writes.push(quote!(.write_u32(self.version)));
            writes.push(quote!(.write_u32(self.#name)));
        } else {
            let name = safe_ident(&arg.name);
            let ty = map_field_type(arg, &interface.name);

            fields.push(quote!(pub #name: #ty));
            writes.push(map_writer(arg, protocol, &interface.name));
        }
    }

    let struct_decl = if fields.is_empty() {
        quote!(pub struct #request_struct_name;)
    } else {
        quote! { pub struct #request_struct_name { #(#fields),* } }
    };

    quote! {
        #[derive(Debug)]
        #struct_decl

        impl WaylandSend for #request_struct_name {
            type Interface = super::#struct_name;
            const OPCODE: u16 = #opcode;

            fn serialize(&self, builder: MessageBuilder) {
                builder #(#writes)* .build();
            }
        }
    }
}
