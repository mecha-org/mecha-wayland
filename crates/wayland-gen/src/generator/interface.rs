use crate::ast::{Interface, Protocol};
use crate::generator::{enums, event, request, utils::*};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

pub fn generate(interface: &Interface, protocol: &Protocol) -> TokenStream {
    let struct_name = format_ident!("{}", to_pascal_case(&interface.name));
    let iface_name_str = &interface.name;
    let version = interface.version;

    // Sub-components
    let generated_enums = enums::generate_enums(interface);
    let events_module = event::generate_events(interface, protocol);
    let requests_module = request::generate_requests(interface, protocol);

    // Assemble the complete interface module
    quote! {
        use crate::proto::{Handle, WaylandInterface, WaylandParse, WaylandSend};
        use crate::wire::{MessageBuilder, MessageReader};

        pub struct #struct_name;

        impl WaylandInterface for #struct_name {
            const NAME: &'static str = #iface_name_str;
            const VERSION: u32 = #version;
        }

        #generated_enums
        #events_module
        #requests_module
    }
}
