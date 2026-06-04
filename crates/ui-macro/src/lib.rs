use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{Field, Fields, Ident, ItemStruct, parse::Parse, parse::ParseStream, parse_macro_input, parse_quote};

enum WidgetArg {
    None,
    Measure,
}

impl Parse for WidgetArg {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        if input.is_empty() {
            return Ok(WidgetArg::None);
        }
        let ident: Ident = input.parse()?;
        match ident.to_string().as_str() {
            "measure" => Ok(WidgetArg::Measure),
            other => Err(syn::Error::new(ident.span(), format!("unknown #[widget] arg `{other}`; expected `measure`"))),
        }
    }
}

/// Transforms a named struct into a widget.
///
/// Injects private `node_id: taffy::NodeId` and `style: taffy::Style` fields,
/// then generates `impl Widget`, `set_style`, and `mark_dirty`.
///
/// - `#[widget]`            — leaf widget (no measure, no children)
/// - `#[widget(measure)]`   — leaf widget; user must `impl Measure for Foo`; `Foo` must be `Clone + 'static`
/// - `#[widget(child)]`     — annotate a field whose type implements `WidgetList`
///
/// `#[widget(measure)]` and `#[widget(child)]` are mutually exclusive.
#[proc_macro_attribute]
pub fn widget(attr: TokenStream, input: TokenStream) -> TokenStream {
    let arg = parse_macro_input!(attr as WidgetArg);
    let mut item = parse_macro_input!(input as ItemStruct);

    let named = match &mut item.fields {
        Fields::Named(f) => &mut f.named,
        _ => return quote!(compile_error!("#[widget] only supports named structs");).into(),
    };

    // Collect #[widget(child)] field names before stripping attrs.
    let child_fields: Vec<Ident> = named
        .iter()
        .filter(|f| is_widget_child(f))
        .map(|f| f.ident.clone().unwrap())
        .collect();

    if matches!(arg, WidgetArg::Measure) && !child_fields.is_empty() {
        return quote!(compile_error!("#[widget(measure)] and #[widget(child)] are mutually exclusive");).into();
    }

    // Strip #[widget(...)] attrs from every field so they don't appear in output.
    for f in named.iter_mut() {
        f.attrs.retain(|a| !a.path().is_ident("widget"));
    }

    // Inject node_id and style at the front (private).
    let node_id_field: Field = parse_quote!(node_id: ::taffy::NodeId);
    let style_field: Field = parse_quote!(style: ::taffy::Style);
    named.insert(0, style_field);
    named.insert(0, node_id_field);

    let name = &item.ident;
    let (impl_generics, ty_generics, where_clause) = item.generics.split_for_impl();

    let is_measure = matches!(arg, WidgetArg::Measure);
    let build_tree_body = build_tree_body(&child_fields, is_measure);
    let render_node_body = render_node_body(&child_fields);

    quote! {
        #item

        impl #impl_generics ::ui::Widget for #name #ty_generics #where_clause {
            fn node_id(&self) -> ::taffy::NodeId { self.node_id }
            fn style(&self) -> &::taffy::Style { &self.style }

            fn build_tree(&mut self, tree: &mut ::ui::WidgetTree) -> ::taffy::NodeId {
                #build_tree_body
            }

            fn render_node(
                &self,
                layout: &::taffy::Layout,
                tree: &::ui::WidgetTree,
                offset: ::ui::Point,
            ) -> ::std::vec::Vec<::ui::RenderCommand> {
                #render_node_body
            }
        }

        impl #impl_generics #name #ty_generics #where_clause {
            pub fn set_style(&mut self, tree: &mut ::ui::WidgetTree, style: ::taffy::Style) {
                tree.set_style(self.node_id, style.clone()).unwrap();
                self.style = style;
                tree.mark_dirty(self.node_id).unwrap();
            }

            pub fn mark_dirty(&self, tree: &mut ::ui::WidgetTree) {
                tree.mark_dirty(self.node_id).unwrap();
            }
        }
    }
    .into()
}

fn is_widget_child(field: &Field) -> bool {
    field.attrs.iter().any(|a| {
        a.path().is_ident("widget")
            && a.parse_args::<Ident>().map_or(false, |i| i == "child")
    })
}

fn build_tree_body(child_fields: &[Ident], is_measure: bool) -> TokenStream2 {
    if !child_fields.is_empty() {
        quote! {
            let mut __ids: ::std::vec::Vec<::taffy::NodeId> = ::std::vec::Vec::new();
            #(
                __ids.extend(::ui::WidgetList::build_children(&mut self.#child_fields, tree));
            )*
            let id = tree.new_with_children(self.style.clone(), &__ids).unwrap();
            self.node_id = id;
            id
        }
    } else if is_measure {
        quote! {
            let context = ::std::boxed::Box::new(self.clone()) as ::std::boxed::Box<dyn ::ui::Measure>;
            let id = tree.new_leaf_with_context(self.style.clone(), context).unwrap();
            self.node_id = id;
            id
        }
    } else {
        quote! {
            let id = tree.new_leaf(self.style.clone()).unwrap();
            self.node_id = id;
            id
        }
    }
}

fn render_node_body(child_fields: &[Ident]) -> TokenStream2 {
    if child_fields.is_empty() {
        quote! {
            let __abs = ::ui::Point::new(offset.x() + layout.location.x, offset.y() + layout.location.y);
            self.render(layout, __abs)
        }
    } else {
        quote! {
            let __abs = ::ui::Point::new(offset.x() + layout.location.x, offset.y() + layout.location.y);
            let mut __cmds = self.render(layout, __abs);
            #(
                __cmds.extend(::ui::WidgetList::render_children(&self.#child_fields, tree, __abs));
            )*
            __cmds
        }
    }
}
