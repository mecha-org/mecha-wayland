use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{
    Data, DeriveInput, Expr, Fields, Ident, Pat, Token, Type,
    parse::{Parse, ParseStream},
    parse_macro_input,
};

/// Derives [`Lens<T>`](app::Lens) for every named field in a struct.
///
/// For a struct `Foo { bar: Bar, baz: Baz }`, this generates:
///
/// ```rust,ignore
/// unsafe impl Lens<Bar> for Foo { fn lens(&mut self) -> &mut Bar { &mut self.bar } }
/// unsafe impl Lens<Baz> for Foo { fn lens(&mut self) -> &mut Baz { &mut self.baz } }
/// ```
///
/// This lets you call `App::new(foo).mount(module)` for any module whose state
/// type is a field of `Foo`, without writing `Lens` impls by hand.
///
/// Only structs with named fields are supported.
#[proc_macro_derive(State, attributes(lens))]
pub fn derive_state(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let fields = match &input.data {
        Data::Struct(s) => match &s.fields {
            Fields::Named(f) => &f.named,
            _ => panic!("#[derive(State)] only supports named fields"),
        },
        _ => panic!("#[derive(State)] only supports structs"),
    };

    let impls: TokenStream2 = fields
        .iter()
        .filter(|f| {
            !f.attrs.iter().any(|a| {
                a.path().is_ident("lens")
                    && a.parse_args::<Ident>().map_or(false, |i| i == "skip")
            })
        })
        .map(|f| {
            let field_name = f.ident.as_ref().unwrap();
            let field_ty = &f.ty;
            quote! {
                unsafe impl #impl_generics ::app::Lens<#field_ty> for #name #ty_generics #where_clause {
                    fn lens(&mut self) -> &mut #field_ty {
                        &mut self.#field_name
                    }
                }
            }
        })
        .collect();

    impls.into()
}

// Placeholder — the real macro is the #[context] attribute below.
#[proc_macro_derive(Context)]
pub fn derive_context_unused(_input: TokenStream) -> TokenStream {
    TokenStream::new()
}

/// Transforms a plain struct into a context struct of `&'a mut` references and
/// generates an `unsafe impl Compose` for it.
///
/// Given:
///
/// ```rust,ignore
/// #[context]
/// struct RenderCtx {
///     renderer: Renderer,
///     wayland: Wayland,
/// }
/// ```
///
/// The macro rewrites the struct to:
///
/// ```rust,ignore
/// struct RenderCtx<'a> {
///     renderer: &'a mut Renderer,
///     wayland:  &'a mut Wayland,
/// }
/// ```
///
/// and generates an `unsafe impl<'a, S> Compose<'a, S> for RenderCtx<'a>` that
/// splits `state` into disjoint mutable references via `Lens`. Using the same
/// field type more than once is a compile error.
///
/// Use [`with_context!`] to produce a closure compatible with [`Module::on`].
#[proc_macro_attribute]
pub fn context(_attr: TokenStream, input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;

    let fields = match &input.data {
        Data::Struct(s) => match &s.fields {
            Fields::Named(f) => f.named.clone(),
            _ => panic!("#[context] only supports named fields"),
        },
        _ => panic!("#[context] only supports structs"),
    };

    // Duplicate type check
    let types: Vec<_> = fields.iter().map(|f| &f.ty).collect();
    for (i, ty) in types.iter().enumerate() {
        for other in types.iter().skip(i + 1) {
            if quote!(#ty).to_string() == quote!(#other).to_string() {
                let msg = format!(
                    "Context field type `{}` appears more than once",
                    quote!(#ty)
                );
                return quote!(compile_error!(#msg);).into();
            }
        }
    }

    let field_names: Vec<_> = fields.iter().map(|f| f.ident.as_ref().unwrap()).collect();
    let field_types: Vec<_> = fields.iter().map(|f| &f.ty).collect();

    let rewritten_struct = quote! {
        struct #name<'a> {
            #( #field_names: &'a mut #field_types, )*
        }
    };

    let field_inits: Vec<_> = field_names
        .iter()
        .zip(field_types.iter())
        .map(|(fname, ty)| {
            quote! {
                #fname: &mut *(::app::Lens::<#ty>::lens(state) as *mut #ty),
            }
        })
        .collect();

    let compose_impl = quote! {
        unsafe impl<'a, S> ::app::Compose<'a, S> for #name<'a>
        where
            S: 'a + #( ::app::Lens<#field_types> + )*,
        {
            fn compose(state: &'a mut S) -> Self {
                unsafe {
                    #name {
                        #( #field_inits )*
                    }
                }
            }
        }
    };

    quote! {
        #rewritten_struct
        #compose_impl
    }
    .into()
}

// ── with_context! ─────────────────────────────────────────────────────────────

struct WithContextInput {
    ctx_ident: Ident,
    ctx_type: Type,
    evt_pat: Pat,
    evt_type: Type,
    body: Expr,
}

impl Parse for WithContextInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        input.parse::<Token![|]>()?;

        // First param: ctx_ident: CtxType
        let ctx_ident: Ident = input.parse()?;
        input.parse::<Token![:]>()?;
        let ctx_type: Type = input.parse()?;
        input.parse::<Token![,]>()?;

        // Second param: ident_or_wildcard: &EventType
        let evt_pat: Pat = Pat::parse_single(input)?;
        input.parse::<Token![:]>()?;
        input.parse::<Token![&]>()?;
        let evt_type: Type = input.parse()?;

        input.parse::<Token![|]>()?;

        let body: Expr = input.parse()?;

        Ok(WithContextInput {
            ctx_ident,
            ctx_type,
            evt_pat,
            evt_type,
            body,
        })
    }
}

/// Wraps a context closure for use with [`Module::on`], composing the context
/// from module state automatically. Emits explicit type annotations so
/// rust-analyzer can provide full LSP support inside the closure body.
///
/// # Example
///
/// ```rust,ignore
/// module.on(with_context!(|ctx: RenderCtx<'_>, e: &Tick| {
///     ctx.renderer.draw_calls += 1;
/// }))
/// ```
#[proc_macro]
pub fn with_context(input: TokenStream) -> TokenStream {
    let WithContextInput {
        ctx_ident,
        ctx_type,
        evt_pat,
        evt_type,
        body,
    } = parse_macro_input!(input as WithContextInput);

    quote! {
        move |__state: &mut _, __event: &_| {
            let #ctx_ident: #ctx_type = ::app::Compose::compose(__state);
            let #evt_pat: &#evt_type = __event;
            #body
        }
    }
    .into()
}
