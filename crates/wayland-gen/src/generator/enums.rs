use crate::ast::{EnumDef, Interface};
use crate::generator::utils::to_pascal_case;
use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote};

pub fn generate_enums(interface: &Interface) -> TokenStream {
    let struct_name = to_pascal_case(&interface.name);

    let enums = interface.enums().map(|enum_def| {
        let enum_name = format_ident!("{}{}", struct_name, to_pascal_case(&enum_def.name));

        if enum_def.bitfield {
            generate_bitfield(&enum_name, enum_def)
        } else {
            generate_enum(&enum_name, enum_def)
        }
    });

    quote! { #(#enums)* }
}

fn generate_bitfield(enum_name: &Ident, enum_def: &EnumDef) -> TokenStream {
    let variants = enum_def.entries.iter().map(|entry| {
        let variant_name = format_variant_ident(&entry.name);
        let value = parse_enum_value(&entry.value);

        quote! {const #variant_name = #value;}
    });

    quote! {
        bitflags::bitflags! {
            #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
            pub struct #enum_name: u32 {
                #(#variants)*
            }
        }
    }
}

fn generate_enum(enum_name: &Ident, enum_def: &EnumDef) -> TokenStream {
    let mut variants = Vec::new();
    let mut from_arms = Vec::new();
    let mut to_arms = Vec::new();

    for entry in &enum_def.entries {
        let variant_name = format_variant_ident(&entry.name);
        let value = parse_enum_value(&entry.value);

        variants.push(quote! { #variant_name, });
        from_arms.push(quote! { #value => Self::#variant_name, });
        to_arms.push(quote! { Self::#variant_name => #value, });
    }

    quote! {
        #[derive(Debug, Copy, Clone, PartialEq, Eq)]
        #[non_exhaustive]
        pub enum #enum_name {
            #(#variants)*
            Unrecognized(u32),
        }

        impl #enum_name {
            pub fn from_wire(val: u32) -> Self {
                match val {
                    #(#from_arms)*
                    _ => Self::Unrecognized(val),
                }
            }

            pub fn to_wire(self) -> u32 {
                match self {
                    #(#to_arms)*
                    Self::Unrecognized(v) => v,
                }
            }
        }
    }
}

fn format_variant_ident(name: &str) -> Ident {
    let mut variant_str = to_pascal_case(name);
    if variant_str.starts_with(|c: char| c.is_ascii_digit()) {
        variant_str.insert(0, '_');
    }
    format_ident!("{}", variant_str)
}

fn parse_enum_value(value: &str) -> TokenStream {
    value
        .parse()
        .expect("Invalid Wayland XML enum/bitfield value")
}
