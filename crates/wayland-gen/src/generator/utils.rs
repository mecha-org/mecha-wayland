use heck::ToPascalCase;
use proc_macro2::Ident;
use syn::parse_str;

pub fn to_pascal_case(s: &str) -> String {
    s.to_pascal_case()
}

pub fn safe_ident(s: &str) -> Ident {
    parse_str::<Ident>(s).unwrap_or_else(|_| {
        let fixed = format!("{s}_");

        parse_str::<Ident>(&fixed).unwrap_or_else(|_| {
            panic!("Invalid Wayland protocol identifier: original=`{s}`, sanitized=`{fixed}`")
        })
    })
}
