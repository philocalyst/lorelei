//! WGSL integer semantics for the Rust (CPU) expansion of a `#[wgsl]` module.
//!
//! WGSL's integer `+ - *` and unary `-` wrap and its shifts take the amount
//! modulo the bit width; Rust's operators panic on overflow when overflow
//! checks are on. There is no per-module overflow-check switch in rustc, so
//! the expansion rewrites the arithmetic instead: in the module's Rust items,
//! after the WGSL has been built from the unmodified source,
//!
//! * `a + b`, `a - b`, `a * b`, `a << b`, `a >> b` become
//!   `<crate>::std::wrap::{add, sub, mul, shl, shr}(a, b)`;
//! * `p += b` (and `-= *= <<= >>=`) becomes
//!   `{ let r = b; <crate>::std::wrap::add_assign(&mut p, r) }` (the right side
//!   first, as for Rust's primitive compound assignment, so a borrow it takes
//!   ends before the place is borrowed);
//! * `-a` becomes `<crate>::std::wrap::neg(a)`.
//!
//! Left alone: operations whose operands are all literals (constants, so an
//! overflow is a compile error either way, and `-2147483648i32` stays a
//! literal), and everything that must remain a constant expression or is not
//! shader code: `const` / `static` items, `const fn`s, `const` blocks, array
//! lengths, types, generic arguments, patterns, enum discriminants, macro
//! arguments (opaque tokens), and items marked `#[wgsl_ignore]`. Division and
//! remainder are not rewritten (see `wgsl_rs::std::wrap`).
use syn::{
    BinOp, Expr, UnOp,
    spanned::Spanned,
    visit_mut::{self, VisitMut},
};

pub(crate) struct WrapArith<'a> {
    pub crate_path: &'a syn::Path,
}

fn is_ignored(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| {
        a.path()
            .segments
            .last()
            .is_some_and(|s| s.ident == "wgsl_ignore")
    })
}

/// A literal, possibly parenthesised or negated: a constant whose type comes
/// from its suffix or from the other operand.
fn is_literal(e: &Expr) -> bool {
    match e {
        Expr::Lit(_) => true,
        Expr::Paren(p) => is_literal(&p.expr),
        Expr::Group(g) => is_literal(&g.expr),
        Expr::Unary(u) => matches!(u.op, UnOp::Neg(_)) && is_literal(&u.expr),
        _ => false,
    }
}

impl VisitMut for WrapArith<'_> {
    fn visit_expr_mut(&mut self, e: &mut Expr) {
        visit_mut::visit_expr_mut(self, e);
        let p = self.crate_path;
        let new: Expr = match e {
            Expr::Binary(b) => {
                if is_literal(&b.left) && is_literal(&b.right) {
                    return;
                }
                let (l, r) = (&b.left, &b.right);
                let span = b.op.span();
                let f = match b.op {
                    BinOp::Add(_) => "add",
                    BinOp::Sub(_) => "sub",
                    BinOp::Mul(_) => "mul",
                    BinOp::Shl(_) => "shl",
                    BinOp::Shr(_) => "shr",
                    BinOp::AddAssign(_) => "add_assign",
                    BinOp::SubAssign(_) => "sub_assign",
                    BinOp::MulAssign(_) => "mul_assign",
                    BinOp::ShlAssign(_) => "shl_assign",
                    BinOp::ShrAssign(_) => "shr_assign",
                    _ => return,
                };
                let f = syn::Ident::new(f, span);
                let attrs = &b.attrs;
                if f.to_string().ends_with("_assign") {
                    let rhs = syn::Ident::new("__wgsl_wrap_rhs", proc_macro2::Span::mixed_site());
                    syn::parse_quote_spanned! {span=>
                        #(#attrs)* { let #rhs = #r; #p::std::wrap::#f(&mut #l, #rhs) }
                    }
                } else {
                    syn::parse_quote_spanned! {span=> #(#attrs)* #p::std::wrap::#f(#l, #r) }
                }
            }
            Expr::Unary(u) if matches!(u.op, UnOp::Neg(_)) && !is_literal(&u.expr) => {
                let (x, attrs) = (&u.expr, &u.attrs);
                let span = u.op.span();
                syn::parse_quote_spanned! {span=> #(#attrs)* #p::std::wrap::neg(#x) }
            }
            _ => return,
        };
        *e = new;
    }

    // Constant contexts and non-shader items: not rewritten.
    fn visit_item_mut(&mut self, i: &mut syn::Item) {
        match i {
            syn::Item::Const(_) | syn::Item::Static(_) | syn::Item::Macro(_) => {}
            syn::Item::Fn(f) if f.sig.constness.is_some() || is_ignored(&f.attrs) => {}
            syn::Item::Impl(x) if is_ignored(&x.attrs) => {}
            syn::Item::Trait(x) if is_ignored(&x.attrs) => {}
            syn::Item::Mod(x) if is_ignored(&x.attrs) => {}
            syn::Item::Struct(_)
            | syn::Item::Enum(_)
            | syn::Item::Union(_)
            | syn::Item::Type(_) => {}
            _ => visit_mut::visit_item_mut(self, i),
        }
    }
    fn visit_impl_item_mut(&mut self, i: &mut syn::ImplItem) {
        match i {
            syn::ImplItem::Const(_) | syn::ImplItem::Type(_) | syn::ImplItem::Macro(_) => {}
            syn::ImplItem::Fn(f) if f.sig.constness.is_some() || is_ignored(&f.attrs) => {}
            _ => visit_mut::visit_impl_item_mut(self, i),
        }
    }
    fn visit_trait_item_mut(&mut self, i: &mut syn::TraitItem) {
        match i {
            syn::TraitItem::Fn(f) if f.sig.constness.is_none() && !is_ignored(&f.attrs) => {
                visit_mut::visit_trait_item_fn_mut(self, f)
            }
            _ => {}
        }
    }
    fn visit_local_mut(&mut self, l: &mut syn::Local) {
        // the pattern (and its type) stays; the initialiser and `else` are code
        if let Some(init) = &mut l.init {
            self.visit_expr_mut(&mut init.expr);
            if let Some((_, d)) = &mut init.diverge {
                self.visit_expr_mut(d);
            }
        }
    }
    fn visit_expr_const_mut(&mut self, _: &mut syn::ExprConst) {}
    fn visit_expr_repeat_mut(&mut self, r: &mut syn::ExprRepeat) {
        self.visit_expr_mut(&mut r.expr);
    }
    fn visit_type_mut(&mut self, _: &mut syn::Type) {}
    fn visit_generic_argument_mut(&mut self, _: &mut syn::GenericArgument) {}
    fn visit_generics_mut(&mut self, _: &mut syn::Generics) {}
    fn visit_pat_mut(&mut self, _: &mut syn::Pat) {}
    fn visit_signature_mut(&mut self, _: &mut syn::Signature) {}
}
