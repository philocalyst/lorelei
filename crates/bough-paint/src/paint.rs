//! Paints as types. A paint is a primitive whose operands are expressions, or a composition of paints
//! (`Over`, `Clip`, `Hit`). The value of a paint type is one record. Its type fixes, before the program
//! runs, everything the generic kernel discovered per command per tile: which primitives, in which order,
//! which operands move its window, whether its core replaces what is beneath, and what it costs.
//!
//! A paint describes itself by continuation: `paint` evaluates its operands and drives a `Sink` with the
//! staged primitives, in paint order, the kind of each call fixed by the type. The tile rasteriser, the
//! flat-row classifier, the window pass, the per-pixel specification, the oracle's stager and the pick are
//! six sinks; each is monomorphised per archetype, so none of them tests a kind.
use crate::expr::{Alpha, Code, Cx, Expr, Ty, U_TIME, Val, Words, time_range};
use bough_expr::latest;
use crate::ops::ops as x;
use bough_kernel::Cmd;
use wgsl_rs::std::{Vec4, Vec4i, Vec4u};

pub const EMPTY: Vec4i = Vec4 {
    x: 0,
    y: 0,
    z: 0,
    w: 0,
};
/// No clip (Q8), the kernel stager's bounds.
pub const NO_CLIP: Vec4i = Vec4 {
    x: -4_096_000,
    y: -4_096_000,
    z: 4_096_000,
    w: 4_096_000,
};
pub const NO_HIT: u32 = u32::MAX;

/// Per-pixel cost class of a paint (the plan's input; `plan.rs`).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Cost {
    /// Constant spans (solids, opaque interiors, glyph rows): a CPU core beats a GPU round trip.
    Flat = 0,
    /// A constant interior with a distance evaluated along the edges (rounded boxes, borders).
    Edge = 1,
    /// Arithmetic at every pixel (gradients).
    PerPixel = 2,
    /// A neighbourhood integrated at every pixel (blurred shadows).
    Wide = 3,
}
impl Cost {
    /// Relative time of one tile entry of this class on a CPU core (from the canvas's per-column profile:
    /// a flat entry about 0.3 us, a blurred one about 7).
    pub const fn weight(self) -> u32 {
        match self {
            Cost::Flat => 1,
            Cost::Edge => 2,
            Cost::PerPixel => 6,
            Cost::Wide => 24,
        }
    }
}
pub const fn worst(a: Cost, b: Cost) -> Cost {
    if a as u8 >= b as u8 { a } else { b }
}

/// A paint's structure as data.
#[derive(Debug)]
pub enum PCode {
    /// `p(mk(args.., clip), x, y)`; `glyph`: `p` also takes the atlas byte at `x_texel(mk(..), x, y, aw)`.
    Prim {
        mk: &'static str,
        p: &'static str,
        args: &'static [&'static Code],
        tys: &'static [Ty],
        glyph: bool,
        /// Its core covers when the colour operands `full` names (indices into `args`) all have full alpha.
        opaque: bool,
        full: &'static [usize],
        /// Its last operand is a table handle (`Shadow`): the generated function reads the corner table.
        tab: bool,
        /// What it reads besides its own record, through hand-written glue (`metal.rs`, `tile.rs`).
        glue: Glue,
    },
    /// `a` then `b`; `cut`: `b`'s core is opaque, so `a` is skipped inside it.
    Over {
        a: &'static PCode,
        b: &'static PCode,
        cut: bool,
    },
    Clip {
        p: &'static PCode,
        k: &'static Code,
    },
    Hit {
        p: &'static PCode,
        id: &'static Code,
    },
}

/// A resource a primitive reads that is not its record: the executor's glue fetches it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Glue {
    None,
    /// The layer the record owns: the flattened prefix beneath it and its pyramid (`layer.rs`).
    Backdrop,
    /// A leased frame of the media pool (`media.rs`).
    Video,
}

/// Where a paint's staged primitives go. Every method has the kind in its name: the caller is a paint
/// type, so the callee is chosen when the archetype is compiled.
pub trait Sink {
    fn solid(&mut self, c: Cmd);
    fn rrect(&mut self, c: Cmd);
    fn border(&mut self, c: Cmd);
    fn shadow(&mut self, c: Cmd);
    fn grad(&mut self, c: Cmd);
    fn glyph(&mut self, c: Cmd);
    /// A backdrop: over the whole tiles of its halo it flattens the accumulator to 8 bits (what its layer
    /// stores), and inside its window it mixes that with the blurred prefix through its filter, so a sink
    /// that blends needs the layer, not only the command.
    fn backdrop(&mut self, c: Cmd);
    fn video(&mut self, c: Cmd);
    /// A user shader node (`ops::p_user`).
    fn user(&mut self, c: Cmd);
    /// The pixels of `core` are about to be replaced by an opaque layer of the same record: what is painted
    /// until `show` may skip them. Returns what to hand back to `show`.
    #[inline(always)]
    fn hide(&mut self, _core: Vec4i) -> Vec4i {
        EMPTY
    }
    #[inline(always)]
    fn show(&mut self, _prev: Vec4i) {}
}

pub trait Paint: Words {
    const READS: u64;
    /// Block words that can move its window (rect, radius, blur and clip operands).
    const GEOM_READS: u64;
    /// Some layer's core is opaque: it covers what is beneath (the occlusion cut's condition, from types).
    const OPAQUE_CORE: bool;
    const COST: Cost;
    const LAYERS: usize;
    const HIT: bool = false;
    /// Some layer is a backdrop: the record owns a layer (`layer.rs`). Such a primitive must be the record's
    /// first (bottom) layer, so "beneath the primitive" is "beneath the record".
    const LAYER: bool = false;
    const CODE: &'static PCode;
    /// From which value of `Time` on the record no longer changes with time alone (`Expr::until`: 0 at
    /// rest, `u32::MAX` never). A time-only frame visits only the records still ahead of it.
    fn until(&self, cx: &Cx) -> u32;
    /// The pixel window and halo can never leave this box while only `Time` changes.
    /// The box is in frame coordinates before the stager's global clip. `None` leaves motion active.
    fn motion_hull(&self, _cx: &Cx) -> Option<Vec4i> {
        None
    }
    fn paint<Sk: Sink>(&self, cx: &Cx, clip: Vec4i, s: &mut Sk);
    /// The pixels a change of block words `mask` can alter without moving anything: the windows (under
    /// `new`) of the layers one of whose operands has a different value under the two blocks. A hovered
    /// card's shadow is not in it.
    fn changed_win(&self, old: &Cx, new: &Cx, mask: u64, clip: Vec4i) -> Vec4i;
    /// The pixels this record covers opaquely (empty unless `OPAQUE_CORE`).
    fn cover(&self, cx: &Cx, clip: Vec4i) -> Vec4i;
    #[inline(always)]
    fn pick_id(&self, _cx: &Cx) -> u32 {
        NO_HIT
    }
    /// Called once when the record is mounted or rewritten, under a zero block: a layer whose coverage is a
    /// pure function of record constants and the pixel's offset asks `m` for the table of its key and keeps
    /// the handle in its own record word. `m(radius, blur)` returns a handle.
    #[inline(always)]
    fn memo(&mut self, _cx: &Cx, _m: &mut dyn FnMut(i32, i32) -> u32) {}
    /// Give each eligible shadow a retained coverage handle where space permits.
    fn memo_coverage(&mut self, _cx: &Cx, _patch: &mut dyn FnMut(Cmd) -> Option<u32>) {}

}

fn swept_window<B: Expr<Out = Vec4i>>(b: &B, cx: &Cx, margin: i32) -> Option<Vec4i> {
    let (lo, hi) = time_range(b, cx)?;
    let lower = |q: i32| Some(q.checked_sub(margin)?.checked_add(127)? >> 8);
    let upper = |q: i32| Some(q.checked_add(margin)?.checked_add(127)? >> 8);
    Some(Vec4 { x: lower(lo.x)?, y: lower(lo.y)?, z: upper(hi.z)?, w: upper(hi.w)? })
}

/// A primitive from its stager leaf and its shade leaf:
/// `prim!(Name: sink_method = mk_leaf, p_leaf(field: Type: Out: moves_window, ..); full = [colour operands
/// whose full alpha makes the core a cover, with their positions]; cover = ..; cost = ..)`.
macro_rules! prim {
    ($(#[$m:meta])* $name:ident : $sink:ident = $mk:ident, $p:ident ( $($f:ident : $T:ident : $ty:ty : $geom:literal),* ); full = [$($ff:ident : $FT:ident = $fi:literal),*]; cover = $cov:literal; cost = $cost:expr; glyph = $gl:literal; window = $b:ident + $margin:literal) => {
        $(#[$m])*
        #[repr(C)]
        #[derive(Clone, Copy)]
        pub struct $name<$($T),*> { $(pub $f: $T),* }
        // SAFETY: `repr(C)` over `Words` operands.
        unsafe impl<$($T: Words),*> Words for $name<$($T),*> {}
        impl<$($T: Expr<Out = $ty> + Words),*> Paint for $name<$($T),*>
        where
            $($FT: Alpha,)*
        {
            const READS: u64 = 0 $(| $T::READS)*;
            const GEOM_READS: u64 = 0 $(| if $geom { $T::READS } else { 0 })*;
            const OPAQUE_CORE: bool = $cov $(&& $FT::MAY)*;
            const COST: Cost = $cost;
            const LAYERS: usize = 1;
            const CODE: &'static PCode = &PCode::Prim { mk: stringify!($mk), p: stringify!($p), args: &[$($T::CODE),*], tys: &[$(<$ty as Val>::TY),*], glyph: $gl, opaque: Self::OPAQUE_CORE, full: &[$($fi),*], tab: false, glue: Glue::None };
            fn motion_hull(&self, cx: &Cx) -> Option<Vec4i> {
                swept_window(&self.$b, cx, $margin)
            }
            #[inline(always)]
            fn paint<Sk: Sink>(&self, cx: &Cx, clip: Vec4i, s: &mut Sk) {
                s.$sink(x::$mk($(self.$f.eval(cx)),*, clip))
            }
            #[inline(always)]
            fn until(&self, cx: &Cx) -> u32 {
                let (now, t) = (cx.u.w[U_TIME], 0u32);
                $(let t = latest(t, self.$f.until(cx), now);)*
                let _ = now;
                t
            }
            #[inline(always)]
            fn changed_win(&self, old: &Cx, new: &Cx, mask: u64, clip: Vec4i) -> Vec4i {
                if false $(|| ($T::READS & mask != 0 && self.$f.eval(old) != self.$f.eval(new)))* {
                    x::$mk($(self.$f.eval(new)),*, clip).win
                } else {
                    EMPTY
                }
            }
            #[inline(always)]
            fn cover(&self, cx: &Cx, clip: Vec4i) -> Vec4i {
                if Self::OPAQUE_CORE $(&& self.$ff.full(cx))* { x::$mk($(self.$f.eval(cx)),*, clip).core } else { EMPTY }
            }
        }
    };
}

prim!(/// A solid box (no antialiasing: pixels whose centres it holds).
    Solid: solid = mk_solid, p_solid(b: B: Vec4i: true, c: C: Vec4u: false); full = [c: C = 1]; cover = true; cost = Cost::Flat; glyph = false; window = b + 0);
prim!(/// A rounded box.
    RRect: rrect = mk_rrect, p_rrect(b: B: Vec4i: true, r: R: i32: true, c: C: Vec4u: false); full = [c: C = 2]; cover = true; cost = Cost::Edge; glyph = false; window = b + 256);
prim!(/// A rounded box of colour `fill` with a border of width `w` in colour `c` inside its edge.
    Border: border = mk_border, p_border(b: B: Vec4i: true, r: R: i32: true, w: W: i32: true, c: C: Vec4u: false, fill: F: Vec4u: false); full = [fill: F = 4]; cover = true; cost = Cost::Edge; glyph = false; window = b + 256);
/// A table handle operand of a primitive that can memoise part of its coverage (`Shadow`'s corners).
pub trait TabOp: Expr<Out = u32> + Words {
    /// The record owns a handle word.
    const ON: bool;
    fn put(&mut self, _handle: u32) {}
}
/// A handle word in the record: filled in when the record is mounted (`Paint::memo`).
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct Tab(pub u32);
impl Tab {
    pub const NONE: Tab = Tab(x::T_MISS);
}
// SAFETY: one word.
unsafe impl Words for Tab {}
impl Expr for Tab {
    type Out = u32;
    type Stage = crate::expr::Mount;
    const READS: u64 = 0;
    const CODE: &'static Code = &Code::Rec;
    #[inline(always)]
    fn eval(&self, _: &Cx) -> u32 {
        self.0
    }
}
impl TabOp for Tab {
    const ON: bool = true;
    #[inline(always)]
    fn put(&mut self, h: u32) {
        self.0 = h;
    }
}
/// No table: the closed form always (no record word).
pub type NoTab = crate::expr::K<{ x::T_MISS }>;
impl TabOp for NoTab {
    const ON: bool = false;
}

/// The shadow of a rounded box, blurred by a spline of width `blur` (Q8, two sigma). `tab`: `Tab` to let the
/// corner corrections be read from a table shared by every shadow of the same radius and blur, `NoTab`
/// (as a value: `K`) for the closed form only.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Shadow<B, R, S, C, T = NoTab> {
    pub b: B,
    pub r: R,
    pub blur: S,
    pub c: C,
    pub tab: T,
}
// SAFETY: `repr(C)` over `Words` operands.
unsafe impl<B: Words, R: Words, S: Words, C: Words, T: Words> Words for Shadow<B, R, S, C, T> {}
impl<
    B: Expr<Out = Vec4i> + Words,
    R: Expr<Out = i32> + Words,
    S: Expr<Out = i32> + Words,
    C: Expr<Out = Vec4u> + Words,
    T: TabOp,
> Paint for Shadow<B, R, S, C, T>
{
    const READS: u64 = B::READS | R::READS | S::READS | C::READS;
    const GEOM_READS: u64 = B::READS | R::READS | S::READS;
    const OPAQUE_CORE: bool = false;
    const COST: Cost = Cost::Wide;
    const LAYERS: usize = 1;
    const CODE: &'static PCode = &PCode::Prim {
        mk: "mk_shadow",
        p: "p_shadow",
        args: &[B::CODE, R::CODE, S::CODE, C::CODE, T::CODE],
        tys: &[Ty::V4i, Ty::I32, Ty::I32, Ty::V4u, Ty::U32],
        glyph: false,
        opaque: false,
        full: &[],
        tab: T::ON,
        glue: Glue::None,
    };
    fn motion_hull(&self, cx: &Cx) -> Option<Vec4i> {
        let (_, blur) = time_range(&self.blur, cx)?;
        let bw = blur.clamp(16, 32768);
        swept_window(&self.b, cx, bw + bw / 2 + 256)
    }
    #[inline(always)]
    fn until(&self, cx: &Cx) -> u32 {
        let now = cx.u.w[U_TIME];
        latest(latest(self.b.until(cx), self.r.until(cx), now), latest(self.blur.until(cx), self.c.until(cx), now), now)
    }
    #[inline(always)]
    fn paint<Sk: Sink>(&self, cx: &Cx, clip: Vec4i, s: &mut Sk) {
        s.shadow(x::mk_shadow(
            self.b.eval(cx),
            self.r.eval(cx),
            self.blur.eval(cx),
            self.c.eval(cx),
            self.tab.eval(cx),
            clip,
        ))
    }
    #[inline(always)]
    fn changed_win(&self, old: &Cx, new: &Cx, mask: u64, clip: Vec4i) -> Vec4i {
        if (B::READS & mask != 0 && self.b.eval(old) != self.b.eval(new))
            || (R::READS & mask != 0 && self.r.eval(old) != self.r.eval(new))
            || (S::READS & mask != 0 && self.blur.eval(old) != self.blur.eval(new))
            || (C::READS & mask != 0 && self.c.eval(old) != self.c.eval(new))
        {
            x::mk_shadow(
                self.b.eval(new),
                self.r.eval(new),
                self.blur.eval(new),
                self.c.eval(new),
                self.tab.eval(new),
                clip,
            )
            .win
        } else {
            EMPTY
        }
    }
    #[inline(always)]
    fn cover(&self, _: &Cx, _: Vec4i) -> Vec4i {
        EMPTY
    }
    fn memo_coverage(&mut self, cx: &Cx, patch: &mut dyn FnMut(Cmd) -> Option<u32>) {
        if T::ON && R::READS == 0 && S::READS == 0 {
            let clear = Vec4 { x: 0, y: 0, z: 0, w: 0 };
            let c = x::mk_shadow(self.b.eval(cx), self.r.eval(cx), self.blur.eval(cx), clear, x::T_MISS, NO_CLIP);
            if let Some(handle) = patch(c) {
                self.tab.put(handle);
            }
        }
    }
    fn memo(&mut self, cx: &Cx, m: &mut dyn FnMut(i32, i32) -> u32) {
        // radius and blur fixed for the record's life: its corners are one table's. (A box whose size is
        // animated can still change the clamped radius; the key in the table's header is checked while
        // shading, so that only costs the closed form.)
        if T::ON && R::READS == 0 && S::READS == 0 {
            let z = Vec4 {
                x: 0,
                y: 0,
                z: 0,
                w: 0,
            };
            let c = x::mk_shadow(
                self.b.eval(cx),
                self.r.eval(cx),
                self.blur.eval(cx),
                z,
                x::T_MISS,
                NO_CLIP,
            );
            if x::t_regime(c.rad.x, c.rad.y) {
                self.tab.put(m(c.rad.x, c.rad.y));
            }
        }
    }
}
prim!(/// A two-stop linear gradient in a rounded box.
    Grad: grad = mk_grad, p_grad(b: B: Vec4i: true, r: R: i32: true, dx: X: i32: false, dy: Y: i32: false, c: C: Vec4u: false, c2: D: Vec4u: false); full = [c: C = 4, c2: D = 5]; cover = true; cost = Cost::PerPixel; glyph = false; window = b + 256);
prim!(/// One glyph: `b` is its texels' box, `texel` its first texel in the atlas.
    Glyph: glyph = mk_glyph, p_glyph(b: B: Vec4i: true, texel: T: u32: false, c: C: Vec4u: false); full = []; cover = false; cost = Cost::Flat; glyph = true; window = b + 0);

prim!(/// An author shader node filling a rounded box: `id` picks the function (`bough_kernel::fx::user_shade`),
    /// `now` is its clock (any expression: the frame clock, a scaled clock, a constant for a still), `c`
    /// and `c2` its two colours. It is damaged where it shows and only when one of these moves.
    Shader: user = mk_user, p_user(b: B: Vec4i: true, r: R: i32: true, id: I: u32: false, now: N: u32: false, c: C: Vec4u: false, c2: D: Vec4u: false); full = []; cover = false; cost = Cost::PerPixel; glyph = false; window = b + 256);

/// A video frame scaled into a rounded box. `slot` names the leased frame in the media pool
/// (`media::Pool::publish` rewrites this word, which is the whole of "a new frame arrived"), `dims` its luma
/// size (width | height << 16), `op` its opacity (Q15).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Video<B, R, S, M, O> {
    pub b: B,
    pub r: R,
    pub slot: S,
    pub dims: M,
    pub op: O,
}
// SAFETY: `repr(C)` over `Words` operands.
unsafe impl<B: Words, R: Words, S: Words, M: Words, O: Words> Words for Video<B, R, S, M, O> {}
impl<B: Expr<Out = Vec4i> + Words, R: Expr<Out = i32> + Words, S: Expr<Out = u32> + Words, M: Expr<Out = u32> + Words, O: Expr<Out = u32> + Words> Paint for Video<B, R, S, M, O> {
    const READS: u64 = B::READS | R::READS | S::READS | M::READS | O::READS;
    const GEOM_READS: u64 = B::READS | R::READS;
    const OPAQUE_CORE: bool = false;
    const COST: Cost = Cost::PerPixel;
    const LAYERS: usize = 1;
    const CODE: &'static PCode = &PCode::Prim { mk: "mk_video", p: "p_video", args: &[B::CODE, R::CODE, S::CODE, M::CODE, O::CODE], tys: &[Ty::V4i, Ty::I32, Ty::U32, Ty::U32, Ty::U32], glyph: false, opaque: false, full: &[], tab: false, glue: Glue::Video };
    fn motion_hull(&self, cx: &Cx) -> Option<Vec4i> {
        swept_window(&self.b, cx, 256)
    }
    #[inline(always)]
    fn paint<Sk: Sink>(&self, cx: &Cx, clip: Vec4i, s: &mut Sk) {
        s.video(x::mk_video(self.b.eval(cx), self.r.eval(cx), self.slot.eval(cx), self.dims.eval(cx), self.op.eval(cx), clip))
    }
    #[inline(always)]
    fn until(&self, cx: &Cx) -> u32 {
        let now = cx.u.w[U_TIME];
        latest(latest(self.b.until(cx), self.r.until(cx), now), latest(self.slot.until(cx), self.op.until(cx), now), now)
    }
    #[inline(always)]
    fn changed_win(&self, old: &Cx, new: &Cx, mask: u64, clip: Vec4i) -> Vec4i {
        if (B::READS & mask != 0 && self.b.eval(old) != self.b.eval(new)) || (R::READS & mask != 0 && self.r.eval(old) != self.r.eval(new)) || (S::READS & mask != 0 && self.slot.eval(old) != self.slot.eval(new)) || (M::READS & mask != 0 && self.dims.eval(old) != self.dims.eval(new)) || (O::READS & mask != 0 && self.op.eval(old) != self.op.eval(new)) {
            x::mk_video(self.b.eval(new), self.r.eval(new), self.slot.eval(new), self.dims.eval(new), self.op.eval(new), clip).win
        } else {
            EMPTY
        }
    }
    #[inline(always)]
    fn cover(&self, _: &Cx, _: Vec4i) -> Vec4i {
        EMPTY
    }
}

/// What is beneath the rounded box `b`, blurred by `blur` (Q8 px, a standard deviation; 0: sharp) and passed
/// through brightness, saturation and contrast (`f`: a `Vec4u` of Q8 factors, 256 each for none). Frosted
/// glass is `Over(Backdrop { .. }, RRect { translucent tint })`. The record owns a layer (`layer.rs`).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Backdrop<B, R, S, F> {
    pub b: B,
    pub r: R,
    pub blur: S,
    pub f: F,
}
// SAFETY: `repr(C)` over `Words` operands.
unsafe impl<B: Words, R: Words, S: Words, F: Words> Words for Backdrop<B, R, S, F> {}
impl<B: Expr<Out = Vec4i> + Words, R: Expr<Out = i32> + Words, S: Expr<Out = i32> + Words, F: Expr<Out = Vec4u> + Words> Paint for Backdrop<B, R, S, F> {
    const READS: u64 = B::READS | R::READS | S::READS | F::READS;
    const GEOM_READS: u64 = B::READS | R::READS | S::READS;
    const OPAQUE_CORE: bool = false;
    const COST: Cost = Cost::PerPixel;
    const LAYERS: usize = 1;
    const LAYER: bool = true;
    const CODE: &'static PCode = &PCode::Prim { mk: "mk_backdrop", p: "p_bdrop", args: &[B::CODE, R::CODE, S::CODE, F::CODE], tys: &[Ty::V4i, Ty::I32, Ty::I32, Ty::V4u], glyph: false, opaque: false, full: &[], tab: false, glue: Glue::Backdrop };
    fn motion_hull(&self, cx: &Cx) -> Option<Vec4i> {
        let (_, blur) = time_range(&self.blur, cx)?;
        // Increasing blur can only add a coarser read. Derive the halo from the outer window
        // without clamping it to the staging coordinates, since the pan index needs world rows.
        let mut cmd = x::mk_backdrop(EMPTY, 0, blur, Vec4 { x: 256, y: 256, z: 256, w: 0 }, NO_CLIP);
        cmd.win = swept_window(&self.b, cx, 256)?;
        Some(crate::layer::halo(cmd))
    }
    #[inline(always)]
    fn paint<Sk: Sink>(&self, cx: &Cx, clip: Vec4i, s: &mut Sk) {
        s.backdrop(x::mk_backdrop(self.b.eval(cx), self.r.eval(cx), self.blur.eval(cx), self.f.eval(cx), clip))
    }
    #[inline(always)]
    fn until(&self, cx: &Cx) -> u32 {
        let now = cx.u.w[U_TIME];
        latest(latest(self.b.until(cx), self.r.until(cx), now), latest(self.blur.until(cx), self.f.until(cx), now), now)
    }
    #[inline(always)]
    fn changed_win(&self, old: &Cx, new: &Cx, mask: u64, clip: Vec4i) -> Vec4i {
        if (B::READS & mask != 0 && self.b.eval(old) != self.b.eval(new)) || (R::READS & mask != 0 && self.r.eval(old) != self.r.eval(new)) || (S::READS & mask != 0 && self.blur.eval(old) != self.blur.eval(new)) || (F::READS & mask != 0 && self.f.eval(old) != self.f.eval(new)) {
            x::mk_backdrop(self.b.eval(new), self.r.eval(new), self.blur.eval(new), self.f.eval(new), clip).win
        } else {
            EMPTY
        }
    }
    #[inline(always)]
    fn cover(&self, _: &Cx, _: Vec4i) -> Vec4i {
        EMPTY
    }
}
/// A backdrop's three filter factors (Q8: brightness, saturation, contrast) from one record word each.
pub type Filt<A, B, C> = crate::expr::V3<A, B, C>;

/// `A` beneath `B`, one record. Where `B`'s core is opaque, `A` is not evaluated: the cut between a record's
/// own layers is decided by the types, not found by a binner.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Over<A, B>(pub A, pub B);
// SAFETY: `repr(C)` over `Words` operands.
unsafe impl<A: Words, B: Words> Words for Over<A, B> {}
impl<A: Paint, B: Paint> Paint for Over<A, B> {
    const READS: u64 = A::READS | B::READS;
    const GEOM_READS: u64 = A::GEOM_READS | B::GEOM_READS;
    const OPAQUE_CORE: bool = A::OPAQUE_CORE || B::OPAQUE_CORE;
    const COST: Cost = worst(A::COST, B::COST);
    const LAYERS: usize = A::LAYERS + B::LAYERS;
    const HIT: bool = A::HIT || B::HIT;
    const LAYER: bool = {
        assert!(!B::LAYER, "a backdrop must be the bottom layer of its record");
        A::LAYER
    };
    fn motion_hull(&self, cx: &Cx) -> Option<Vec4i> {
        Some(x::x_union(self.0.motion_hull(cx)?, self.1.motion_hull(cx)?))
    }
    #[inline(always)]
    fn until(&self, cx: &Cx) -> u32 {
        latest(self.0.until(cx), self.1.until(cx), cx.u.w[U_TIME])
    }
    const CODE: &'static PCode = &PCode::Over {
        a: A::CODE,
        b: B::CODE,
        cut: B::OPAQUE_CORE,
    };
    #[inline(always)]
    fn paint<Sk: Sink>(&self, cx: &Cx, clip: Vec4i, s: &mut Sk) {
        if B::OPAQUE_CORE {
            let prev = s.hide(self.1.cover(cx, clip));
            self.0.paint(cx, clip, s);
            s.show(prev);
        } else {
            self.0.paint(cx, clip, s);
        }
        self.1.paint(cx, clip, s);
    }
    #[inline(always)]
    fn changed_win(&self, old: &Cx, new: &Cx, mask: u64, clip: Vec4i) -> Vec4i {
        x::x_union(
            self.0.changed_win(old, new, mask, clip),
            self.1.changed_win(old, new, mask, clip),
        )
    }
    #[inline(always)]
    fn cover(&self, cx: &Cx, clip: Vec4i) -> Vec4i {
        // the upper layer's core if it has one; else the lower's is still a cover only where the upper layer
        // does not ink, which is not known here, so a translucent top makes the record no cover
        if B::OPAQUE_CORE {
            self.1.cover(cx, clip)
        } else {
            EMPTY
        }
    }
    #[inline(always)]
    fn pick_id(&self, cx: &Cx) -> u32 {
        if B::HIT {
            self.1.pick_id(cx)
        } else {
            self.0.pick_id(cx)
        }
    }
    #[inline(always)]
    fn memo_coverage(&mut self, cx: &Cx, patch: &mut dyn FnMut(Cmd) -> Option<u32>) {
        self.0.memo_coverage(cx, patch);
        self.1.memo_coverage(cx, patch);
    }
    fn memo(&mut self, cx: &Cx, m: &mut dyn FnMut(i32, i32) -> u32) {
        self.0.memo(cx, m);
        self.1.memo(cx, m);
    }
}

/// `P` cut by a rect expression (Q8).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Clip<P, K> {
    pub p: P,
    pub k: K,
}
// SAFETY: `repr(C)` over `Words` operands.
unsafe impl<P: Words, K: Words> Words for Clip<P, K> {}
impl<P: Paint, K: Expr<Out = Vec4i> + Words> Paint for Clip<P, K> {
    const READS: u64 = P::READS | K::READS;
    const GEOM_READS: u64 = P::GEOM_READS | K::READS;
    const OPAQUE_CORE: bool = P::OPAQUE_CORE;
    const COST: Cost = P::COST;
    const LAYERS: usize = P::LAYERS;
    const HIT: bool = P::HIT;
    const LAYER: bool = P::LAYER;
    fn motion_hull(&self, cx: &Cx) -> Option<Vec4i> {
        let w = self.p.motion_hull(cx)?;
        if P::LAYER {
            // A backdrop flattens over its halo, including outside the clip.
            return Some(w);
        }
        Some(x::x_and(w, swept_window(&self.k, cx, 0)?))
    }
    #[inline(always)]
    fn until(&self, cx: &Cx) -> u32 {
        latest(self.p.until(cx), self.k.until(cx), cx.u.w[U_TIME])
    }
    const CODE: &'static PCode = &PCode::Clip {
        p: P::CODE,
        k: K::CODE,
    };
    #[inline(always)]
    fn paint<Sk: Sink>(&self, cx: &Cx, clip: Vec4i, s: &mut Sk) {
        self.p.paint(cx, x::x_and(clip, self.k.eval(cx)), s)
    }
    #[inline(always)]
    fn changed_win(&self, old: &Cx, new: &Cx, mask: u64, clip: Vec4i) -> Vec4i {
        // a clip that changed moved the window where it matters: that is the window pass's other branch
        self.p
            .changed_win(old, new, mask, x::x_and(clip, self.k.eval(new)))
    }
    #[inline(always)]
    fn cover(&self, cx: &Cx, clip: Vec4i) -> Vec4i {
        self.p.cover(cx, x::x_and(clip, self.k.eval(cx)))
    }
    #[inline(always)]
    fn pick_id(&self, cx: &Cx) -> u32 {
        self.p.pick_id(cx)
    }
    #[inline(always)]
    fn memo_coverage(&mut self, cx: &Cx, patch: &mut dyn FnMut(Cmd) -> Option<u32>) {
        self.p.memo_coverage(cx, patch);
    }
    fn memo(&mut self, cx: &Cx, m: &mut dyn FnMut(i32, i32) -> u32) {
        self.p.memo(cx, m);
    }
}

/// `P`, answering the pick with `id` wherever it inks.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Hit<P, I> {
    pub p: P,
    pub id: I,
}
// SAFETY: `repr(C)` over `Words` operands.
unsafe impl<P: Words, I: Words> Words for Hit<P, I> {}
impl<P: Paint, I: Expr<Out = u32> + Words> Paint for Hit<P, I> {
    const READS: u64 = P::READS | I::READS;
    const GEOM_READS: u64 = P::GEOM_READS;
    const OPAQUE_CORE: bool = P::OPAQUE_CORE;
    const COST: Cost = P::COST;
    const LAYERS: usize = P::LAYERS;
    const HIT: bool = true;
    const LAYER: bool = P::LAYER;
    fn motion_hull(&self, cx: &Cx) -> Option<Vec4i> {
        self.p.motion_hull(cx)
    }
    #[inline(always)]
    fn until(&self, cx: &Cx) -> u32 {
        latest(self.p.until(cx), self.id.until(cx), cx.u.w[U_TIME])
    }
    const CODE: &'static PCode = &PCode::Hit {
        p: P::CODE,
        id: I::CODE,
    };
    #[inline(always)]
    fn paint<Sk: Sink>(&self, cx: &Cx, clip: Vec4i, s: &mut Sk) {
        self.p.paint(cx, clip, s)
    }
    #[inline(always)]
    fn changed_win(&self, old: &Cx, new: &Cx, mask: u64, clip: Vec4i) -> Vec4i {
        self.p.changed_win(old, new, mask, clip)
    }
    #[inline(always)]
    fn cover(&self, cx: &Cx, clip: Vec4i) -> Vec4i {
        self.p.cover(cx, clip)
    }
    #[inline(always)]
    fn pick_id(&self, cx: &Cx) -> u32 {
        self.id.eval(cx)
    }
    #[inline(always)]
    fn memo_coverage(&mut self, cx: &Cx, patch: &mut dyn FnMut(Cmd) -> Option<u32>) {
        self.p.memo_coverage(cx, patch);
    }
    fn memo(&mut self, cx: &Cx, m: &mut dyn FnMut(i32, i32) -> u32) {
        self.p.memo(cx, m);
    }
}

/// The union of the staged primitives' windows (the record's window lane), and the halo of its backdrop
/// (the frame pixels whose prefix its layer keeps: the bins list the record there too).
pub struct WinSink(pub Vec4i, pub Vec4i);
impl WinSink {
    #[inline(always)]
    fn add(&mut self, c: Cmd) {
        self.0 = x::x_union(self.0, c.win);
    }
}
/// The backdrop command of a record that owns a layer.
pub struct LaySink(pub Option<Cmd>);
impl Sink for LaySink {
    #[inline(always)]
    fn solid(&mut self, _: Cmd) {}
    #[inline(always)]
    fn rrect(&mut self, _: Cmd) {}
    #[inline(always)]
    fn border(&mut self, _: Cmd) {}
    #[inline(always)]
    fn shadow(&mut self, _: Cmd) {}
    #[inline(always)]
    fn grad(&mut self, _: Cmd) {}
    #[inline(always)]
    fn glyph(&mut self, _: Cmd) {}
    #[inline(always)]
    fn backdrop(&mut self, c: Cmd) {
        self.0 = Some(c);
    }
    #[inline(always)]
    fn video(&mut self, _: Cmd) {}
    #[inline(always)]
    fn user(&mut self, _: Cmd) {}
}
impl Sink for WinSink {
    #[inline(always)]
    fn backdrop(&mut self, c: Cmd) {
        self.add(c);
        let h = crate::layer::halo(c);
        self.1 = Vec4 { x: h.x.clamp(-32768, 32767), y: h.y.clamp(-32768, 32767), z: h.z.clamp(-32768, 32767), w: h.w.clamp(-32768, 32767) };
    }
    #[inline(always)]
    fn video(&mut self, c: Cmd) {
        self.add(c)
    }
    #[inline(always)]
    fn user(&mut self, c: Cmd) {
        self.add(c)
    }
    #[inline(always)]
    fn solid(&mut self, c: Cmd) {
        self.add(c)
    }
    #[inline(always)]
    fn rrect(&mut self, c: Cmd) {
        self.add(c)
    }
    #[inline(always)]
    fn border(&mut self, c: Cmd) {
        self.add(c)
    }
    #[inline(always)]
    fn shadow(&mut self, c: Cmd) {
        self.add(c)
    }
    #[inline(always)]
    fn grad(&mut self, c: Cmd) {
        self.add(c)
    }
    #[inline(always)]
    fn glyph(&mut self, c: Cmd) {
        self.add(c)
    }
}

/// The specification of an archetype at one pixel: every layer through its guarded shade leaf, folded with
/// the kernel's blend. No core, no span, no hidden region. This is what the generated GPU function computes.
pub struct SpecSink<'a> {
    pub x: i32,
    pub y: i32,
    pub acc: Vec4u,
    pub atlas: &'a [u8],
    pub aw: u32,
    /// The corner tables.
    pub tabs: &'a [u32],
    /// Some layer inked this pixel (the pick's test).
    pub ink: bool,
    /// The blurred prefix a backdrop command reads at this pixel (the caller's reference pyramid).
    pub bd: &'a dyn Fn(Cmd, i32, i32) -> Vec4u,
    pub media: &'a [u32],
}
/// Resolve a retained coverage patch. A changed key means the record uses the analytic profile.
pub(crate) fn coverage_patch(c: Cmd, tabs: &[u32]) -> Option<(Vec4i, &[u32])> {
    let handle = c.par.z;
    if handle & x::PATCH_BIT == 0 {
        return None;
    }
    let table = tabs.get((handle & x::PATCH_MASK) as usize..)?;
    let [r, w, phase, dims] = *table.first_chunk::<4>()?;
    let s = x::x_patch_shape(c);
    if !x::x_patch_ok(c, s, r, w, phase, dims) {
        return None;
    }
    let (width, height) = (s.x + 2 * s.w + 1, s.y + 2 * s.w + 1);
    Some((s, table.get(4..4 + (width as usize * height as usize).div_ceil(2))?))
}

/// The four corner taps of a shadow at one pixel, read from `tabs`.
#[inline(always)]
pub fn corner_taps(c: Cmd, x: i32, y: i32, ok: bool, tabs: &[u32]) -> [u32; 4] {
    let base = c.par.z as usize;
    let t = x::x_ctaps(c, x, y, ok);
    let mut v = [t.x, t.y, t.z, t.w];
    for v in v.iter_mut() {
        *v = if *v == x::T_ZERO {
            0
        } else if *v == x::T_MISS {
            x::T_MISS
        } else {
            x::t_half(tabs[base + 2 + (*v >> 1) as usize], *v)
        };
    }
    v
}
/// The table named by a shadow's handle is filled for its key.
#[inline(always)]
pub fn tab_ok(c: Cmd, tabs: &[u32]) -> bool {
    let base = c.par.z as usize;
    base.saturating_add(1) < tabs.len() && x::x_tabok(c, tabs[base], tabs[base + 1])
}
impl SpecSink<'_> {
    #[inline(always)]
    fn put(&mut self, sh: Vec4u) {
        self.ink |= sh.w != 0;
        self.acc = x::x_over(self.acc, sh);
    }
}
impl Sink for SpecSink<'_> {
    #[inline(always)]
    fn solid(&mut self, c: Cmd) {
        self.put(x::p_solid(c, self.x, self.y))
    }
    #[inline(always)]
    fn rrect(&mut self, c: Cmd) {
        self.put(x::p_rrect(c, self.x, self.y))
    }
    #[inline(always)]
    fn border(&mut self, c: Cmd) {
        self.put(x::p_border(c, self.x, self.y))
    }
    #[inline(always)]
    fn shadow(&mut self, c: Cmd) {
        self.put(x::p_shadow(c, self.x, self.y))
    }
    #[inline(always)]
    fn grad(&mut self, c: Cmd) {
        self.put(x::p_grad(c, self.x, self.y))
    }
    #[inline(always)]
    fn backdrop(&mut self, c: Cmd) {
        // the halo is whole tiles: wherever the record keeps a prefix, the accumulator is that prefix
        if x::x_in(crate::layer::halo(c), self.x, self.y) {
            self.acc = x::x_flat(self.acc);
        }
        if x::x_in(c.win, self.x, self.y) {
            self.ink = true;
            self.acc = x::p_bdrop(self.acc, c, self.x, self.y, (self.bd)(c, self.x, self.y));
        }
    }
    #[inline(always)]
    fn video(&mut self, c: Cmd) {
        if x::x_in(c.win, self.x, self.y) {
            self.put(x::p_video(c, self.x, self.y, crate::media::sample(self.media, c, self.x, self.y)))
        }
    }
    #[inline(always)]
    fn user(&mut self, c: Cmd) {
        self.put(x::p_user(c, self.x, self.y))
    }
    #[inline(always)]
    fn glyph(&mut self, c: Cmd) {
        let t = x::x_texel(c, self.x, self.y, self.aw);
        let cov = if t == bough_kernel::k::NO_TEXEL || self.atlas.is_empty() {
            0
        } else {
            self.atlas[(t as usize).min(self.atlas.len() - 1)] as u32
        };
        self.put(x::p_glyph(c, self.x, self.y, cov))
    }
}

/// The staged primitives as the generic kernel's commands, in paint order (the oracle's input).
pub struct StageSink<'a>(pub &'a mut bough_kernel::stage::Scene);
impl StageSink<'_> {
    #[inline]
    fn add(&mut self, c: Cmd) {
        if c.win.x < c.win.z {
            self.0.push(c);
        }
    }
}
impl Sink for StageSink<'_> {
    /// The round-7 kernel knows no backdrop and no video: a scene with them has no generic-kernel form
    /// (its oracle is `Scene::spec`). A user node is the kernel's own.
    fn backdrop(&mut self, _: Cmd) {}
    fn video(&mut self, _: Cmd) {}
    fn user(&mut self, c: Cmd) {
        self.add(c)
    }
    fn solid(&mut self, c: Cmd) {
        self.add(c)
    }
    fn rrect(&mut self, c: Cmd) {
        self.add(c)
    }
    fn border(&mut self, c: Cmd) {
        self.add(c)
    }
    fn shadow(&mut self, c: Cmd) {
        self.add(c)
    }
    fn grad(&mut self, c: Cmd) {
        self.add(c)
    }
    fn glyph(&mut self, c: Cmd) {
        self.add(c)
    }
}
