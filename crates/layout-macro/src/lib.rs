use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::quote;
use syn::{
    Expr, Ident, Token, braced,
    parse::{Parse, ParseStream},
    parse_macro_input,
};

// ── DSL prop ─────────────────────────────────────────────────────────────────

struct LayoutProp {
    key: Ident,
    value: Expr,
}

impl Parse for LayoutProp {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let key: Ident = input.parse()?;
        input.parse::<Token![:]>()?;
        let value: Expr = input.parse()?;
        Ok(LayoutProp { key, value })
    }
}

// ── Tree node ─────────────────────────────────────────────────────────────────

struct LayoutNode {
    props: Vec<LayoutProp>,
    children: Vec<LayoutNode>,
    body: TokenStream2,
}

// Parsed direction / justify as compile-time constants
#[derive(Clone, Copy, PartialEq)]
enum Dir {
    Row,
    Column,
}

#[derive(Clone, Copy, PartialEq)]
enum Justify {
    FlexStart,
    SpaceBetween,
}

// ── Parser ────────────────────────────────────────────────────────────────────

struct FirstScope {
    props: Vec<LayoutProp>,
    children: Vec<LayoutNode>,
}

impl Parse for FirstScope {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut props = Vec::new();
        let mut children = Vec::new();

        loop {
            // Skip separators
            while input.peek(Token![,]) || input.peek(Token![;]) {
                if input.peek(Token![,]) {
                    input.parse::<Token![,]>()?;
                } else {
                    input.parse::<Token![;]>()?;
                }
            }
            if input.is_empty() {
                break;
            }

            // Peek for nested layout!
            if input.peek(Ident) {
                let fork = input.fork();
                let ident: Ident = fork.parse()?;
                if ident == "layout" && fork.peek(Token![!]) {
                    children.push(parse_layout_invocation(input)?);
                    continue;
                }
            }

            props.push(input.parse::<LayoutProp>()?);
        }

        Ok(FirstScope { props, children })
    }
}

fn parse_layout_invocation(input: ParseStream) -> syn::Result<LayoutNode> {
    let _: Ident = input.parse()?; // layout
    input.parse::<Token![!]>()?;
    let paren;
    syn::parenthesized!(paren in input);
    parse_layout_args(&paren)
}

fn parse_layout_args(input: ParseStream) -> syn::Result<LayoutNode> {
    let first_content;
    braced!(first_content in input);
    let first: FirstScope = first_content.parse()?;

    input.parse::<Token![,]>()?;

    let second_content;
    braced!(second_content in input);
    let body: TokenStream2 = second_content.parse()?;

    let _ = input.parse::<Token![,]>();

    Ok(LayoutNode {
        props: first.props,
        children: first.children,
        body,
    })
}

impl Parse for LayoutNode {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        parse_layout_args(input)
    }
}

// ── Flat node info ────────────────────────────────────────────────────────────

struct NodeInfo {
    idx: usize,
    dir: Dir,
    justify: Justify,
    width_prop: Option<Expr>,
    height_prop: Option<Expr>,
    available_width: Option<Expr>,
    available_height: Option<Expr>,
    gap: Option<Expr>,
    padding_top: Option<Expr>,
    padding_right: Option<Expr>,
    padding_bottom: Option<Expr>,
    padding_left: Option<Expr>,
    parent_idx: Option<usize>,
    child_indices: Vec<usize>,
    body: TokenStream2,
}

fn flatten(node: LayoutNode, parent: Option<usize>, nodes: &mut Vec<NodeInfo>) -> usize {
    let idx = nodes.len();

    let mut dir = Dir::Row;
    let mut justify = Justify::FlexStart;
    let mut width_prop = None;
    let mut height_prop = None;
    let mut available_width = None;
    let mut available_height = None;
    let mut gap = None;
    let mut padding_top = None;
    let mut padding_right = None;
    let mut padding_bottom = None;
    let mut padding_left = None;

    for p in node.props {
        match p.key.to_string().as_str() {
            "direction" => {
                // Accept bare `row` / `column` idents
                if let Expr::Path(ref ep) = p.value {
                    if let Some(seg) = ep.path.segments.first() {
                        match seg.ident.to_string().as_str() {
                            "row" => dir = Dir::Row,
                            "column" => dir = Dir::Column,
                            _ => {}
                        }
                    }
                }
            }
            "justify" => {
                if let Expr::Path(ref ep) = p.value {
                    if let Some(seg) = ep.path.segments.first() {
                        if seg.ident == "space_between" {
                            justify = Justify::SpaceBetween;
                        }
                    }
                }
            }
            "width" => width_prop = Some(p.value),
            "height" => height_prop = Some(p.value),
            "available_width" => available_width = Some(p.value),
            "available_height" => available_height = Some(p.value),
            "gap" => gap = Some(p.value),
            "padding_top" => padding_top = Some(p.value),
            "padding_right" => padding_right = Some(p.value),
            "padding_bottom" => padding_bottom = Some(p.value),
            "padding_left" => padding_left = Some(p.value),
            _ => {}
        }
    }

    nodes.push(NodeInfo {
        idx,
        dir,
        justify,
        width_prop,
        height_prop,
        available_width,
        available_height,
        gap,
        padding_top,
        padding_right,
        padding_bottom,
        padding_left,
        parent_idx: parent,
        child_indices: Vec::new(),
        body: node.body,
    });

    let mut children = Vec::new();
    for child in node.children {
        children.push(flatten(child, Some(idx), nodes));
    }
    nodes[idx].child_indices = children;

    idx
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn slot_w(i: usize) -> Ident {
    Ident::new(&format!("__lw{i}"), Span::call_site())
}
fn slot_h(i: usize) -> Ident {
    Ident::new(&format!("__lh{i}"), Span::call_site())
}
fn slot_x(i: usize) -> Ident {
    Ident::new(&format!("__lx{i}"), Span::call_site())
}
fn slot_y(i: usize) -> Ident {
    Ident::new(&format!("__ly{i}"), Span::call_site())
}

fn or_zero(e: &Option<Expr>) -> TokenStream2 {
    match e {
        Some(v) => quote! { (#v) as f32 },
        None => quote! { 0.0_f32 },
    }
}

// ── Code generation ───────────────────────────────────────────────────────────

fn emit_solver(nodes: &[NodeInfo]) -> TokenStream2 {
    let mut out = TokenStream2::new();

    // ── Declare all stack slots ──────────────────────────────────────────────
    for n in nodes {
        let (w, h, x, y) = (slot_w(n.idx), slot_h(n.idx), slot_x(n.idx), slot_y(n.idx));
        out.extend(quote! { let #w: f32; let #h: f32; let #x: f32; let #y: f32; });
    }

    // ── Phase 1: Assign explicit / root sizes ────────────────────────────────
    for n in nodes {
        let (w, h) = (slot_w(n.idx), slot_h(n.idx));
        if n.parent_idx.is_none() {
            // Root: size comes from available_width / available_height props
            let aw = or_zero(&n.available_width);
            let ah = or_zero(&n.available_height);
            out.extend(quote! { #w = #aw; #h = #ah; });
        } else {
            if let Some(wp) = &n.width_prop {
                out.extend(quote! { #w = (#wp) as f32; });
            }
            if let Some(hp) = &n.height_prop {
                out.extend(quote! { #h = (#hp) as f32; });
            }
        }
    }

    // ── Phase 2: Fill propagation (pre-order: parents before children) ───────
    // nodes vec is already in pre-order (parent pushed before children)
    for n in nodes {
        if n.child_indices.is_empty() {
            continue;
        }
        let pw = slot_w(n.idx);
        let ph = slot_h(n.idx);
        let pad_l = or_zero(&n.padding_left);
        let pad_r = or_zero(&n.padding_right);
        let pad_t = or_zero(&n.padding_top);
        let pad_b = or_zero(&n.padding_bottom);

        for &ci in &n.child_indices {
            let child = &nodes[ci];
            let cw = slot_w(ci);
            let ch = slot_h(ci);
            match n.dir {
                Dir::Column => {
                    // Cross axis: fill width if no explicit width
                    if child.width_prop.is_none() && child.available_width.is_none() {
                        out.extend(quote! { #cw = #pw - #pad_l - #pad_r; });
                    }
                }
                Dir::Row => {
                    // Cross axis: fill height if no explicit height
                    if child.height_prop.is_none() && child.available_height.is_none() {
                        out.extend(quote! { #ch = #ph - #pad_t - #pad_b; });
                    }
                }
            }
        }
    }

    // ── Phase 3: Positions (pre-order) ───────────────────────────────────────
    // Root is always at (0, 0)
    let (rx, ry) = (slot_x(0), slot_y(0));
    out.extend(quote! { #rx = 0.0_f32; #ry = 0.0_f32; });

    for n in nodes {
        if n.child_indices.is_empty() {
            continue;
        }

        let px = slot_x(n.idx);
        let py = slot_y(n.idx);
        let pw = slot_w(n.idx);
        let ph = slot_h(n.idx);
        let pad_l = or_zero(&n.padding_left);
        let pad_r = or_zero(&n.padding_right);
        let pad_t = or_zero(&n.padding_top);
        let gap = or_zero(&n.gap);

        match n.dir {
            Dir::Column => {
                // Children stacked vertically; x is aligned to left (+ pad_l)
                // y accumulates: parent_y + pad_t, then each child advances by h + gap
                let mut cur_y = quote! { #py + #pad_t };
                for &ci in &n.child_indices {
                    let cx = slot_x(ci);
                    let cy = slot_y(ci);
                    let ch = slot_h(ci);
                    let cur_y_clone = cur_y.clone();
                    out.extend(quote! {
                        #cx = #px + #pad_l;
                        #cy = #cur_y_clone;
                    });
                    cur_y = quote! { #cy + #ch + #gap };
                }
            }
            Dir::Row => match n.justify {
                Justify::FlexStart => {
                    let mut cur_x = quote! { #px + #pad_l };
                    for &ci in &n.child_indices {
                        let cx = slot_x(ci);
                        let cy = slot_y(ci);
                        let cw = slot_w(ci);
                        let ch = slot_h(ci);
                        let cur_x_clone = cur_x.clone();
                        out.extend(quote! {
                            #cx = #cur_x_clone;
                            #cy = #py + (#ph - #ch) / 2.0_f32;
                        });
                        cur_x = quote! { #cx + #cw + #gap };
                    }
                }
                Justify::SpaceBetween => {
                    let n_children = n.child_indices.len();
                    let child_ws: Vec<_> = n.child_indices.iter().map(|&c| slot_w(c)).collect();
                    let space_id = Ident::new(&format!("__lspc{}", n.idx), Span::call_site());
                    let sum_id = Ident::new(&format!("__lsum{}", n.idx), Span::call_site());
                    out.extend(quote! {
                        let #sum_id: f32 = #(#child_ws as f32)+*;
                        let #space_id: f32 = if #n_children > 1 {
                            (#pw - #pad_l - #pad_r - #sum_id) / (#n_children as f32 - 1.0_f32)
                        } else {
                            0.0_f32
                        };
                    });
                    let mut cur_x = quote! { #px + #pad_l };
                    for &ci in &n.child_indices {
                        let cx = slot_x(ci);
                        let cy = slot_y(ci);
                        let cw = slot_w(ci);
                        let ch = slot_h(ci);
                        let cur_x_clone = cur_x.clone();
                        out.extend(quote! {
                            #cx = #cur_x_clone;
                            #cy = #py + (#ph - #ch) / 2.0_f32;
                        });
                        cur_x = quote! { #cx + #cw + #space_id };
                    }
                }
            },
        }
    }

    out
}

fn emit_bodies(nodes: &[NodeInfo]) -> TokenStream2 {
    let mut out = TokenStream2::new();
    for n in nodes {
        let (w, h, x, y) = (slot_w(n.idx), slot_h(n.idx), slot_x(n.idx), slot_y(n.idx));
        let body = &n.body;
        out.extend(quote! {
            {
                let x: f32 = #x;
                let y: f32 = #y;
                let width: f32 = #w;
                let height: f32 = #h;
                #body
            }
        });
    }
    out
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[proc_macro]
pub fn layout(input: TokenStream) -> TokenStream {
    let root: LayoutNode = parse_macro_input!(input as LayoutNode);

    let mut nodes: Vec<NodeInfo> = Vec::new();
    flatten(root, None, &mut nodes);

    let solver = emit_solver(&nodes);
    let bodies = emit_bodies(&nodes);

    quote! { { #solver #bodies } }.into()
}
