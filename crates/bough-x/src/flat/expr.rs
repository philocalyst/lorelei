//! Values and expressions as data.
//!
//! A [`Val`] is what a record's property is bound to: sixteen bytes, `Copy`, no type parameter. Most are a
//! constant or one state word; the rest name a row of a flat table (an animation, or an expression on the
//! [`ExprTape`]). An expression is a straight run of register operations: no stack, no recursion, sixteen
//! registers on the evaluator's own stack frame, so evaluating one allocates nothing and cannot overflow.
use super::state::{DepMask, SlotId, StateBlock};

/// What a property is bound to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Val {
    /// The word itself.
    Const(u32),
    /// A state word, read by index.
    Slot(u16),
    /// Colour `slot` of the palette of the theme whose index is in state word `theme_slot`. On a fill or a
    /// border it is resolved when painted: a theme switch visits no record.
    Palette { slot: u16, theme_slot: u16 },
    /// `on` while the pointer is over hover site `site`, else `off`. A site names its depth in its top byte
    /// (`site >> 24`), and the world keeps one "who is hovered" word per depth: one compare.
    HoverSel { site: u32, off: u32, on: u32 },
    /// `on` while the pointer is over hover site `site`, else palette colour `slot`. On a fill this is the
    /// keyed hover and the lazy palette at once: a hover visits two records, a theme switch none.
    HoverPal { site: u32, slot: u16, theme_slot: u16, on: u32 },
    /// The current value of an animation of the world's table.
    Anim(u32),
    /// The result of an expression of the tape.
    Expr(u32),
}
const _: () = assert!(size_of::<Val>() == 16);

impl Default for Val {
    fn default() -> Val {
        Val::Const(0)
    }
}
impl From<u32> for Val {
    fn from(v: u32) -> Val {
        Val::Const(v)
    }
}
impl From<i32> for Val {
    fn from(v: i32) -> Val {
        Val::Const(v as u32)
    }
}
impl From<bool> for Val {
    fn from(v: bool) -> Val {
        Val::Const(v as u32)
    }
}
impl<T> From<super::state::Slot<T>> for Val {
    fn from(s: super::state::Slot<T>) -> Val {
        Val::Slot(s.id().0)
    }
}
impl From<SlotId> for Val {
    fn from(s: SlotId) -> Val {
        Val::Slot(s.0)
    }
}

/// The depth a hover site belongs to.
#[inline]
pub const fn site_depth(site: u32) -> usize {
    (site >> 24) as usize
}
pub const HOVER_DEPTHS: usize = 8;
/// "No site": what a hover word holds when nothing of its depth is under the pointer.
pub const NO_SITE: u32 = u32::MAX;

/// A register of an expression.
pub type Reg = u8;
pub const REGS: usize = 16;

/// One operation. Words are `u32`; arithmetic reads them as `i32` and wraps. `Q8` operands are fixed point with
/// eight fraction bits (256 is one).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExprOp {
    LoadSlot { dst: Reg, slot: u16 },
    Const { dst: Reg, v: u32 },
    Add { dst: Reg, a: Reg, b: Reg },
    Sub { dst: Reg, a: Reg, b: Reg },
    /// `(a * b) >> 8`.
    MulQ8 { dst: Reg, a: Reg, b: Reg },
    /// `(a << 8) / b`, 0 when `b` is 0.
    DivQ8 { dst: Reg, a: Reg, b: Reg },
    Clamp { dst: Reg, v: Reg, lo: Reg, hi: Reg },
    /// 1 if equal, else 0.
    Eq { dst: Reg, a: Reg, b: Reg },
    /// `a` if `c` is not 0, else `b`.
    Select { dst: Reg, c: Reg, a: Reg, b: Reg },
    /// Per channel `a + (b - a) * t / 256`, `t` clamped to 0 ..= 256.
    LerpColor { dst: Reg, a: Reg, b: Reg, t: Reg },
    LerpI32 { dst: Reg, a: Reg, b: Reg, t: Reg },
    /// `v` taken from `in0 ..= in1` to `out0 ..= out1`, clamped.
    LinearMap { dst: Reg, v: Reg, in0: Reg, in1: Reg, out0: Reg, out1: Reg },
    PaletteLookup { dst: Reg, slot: u16, theme_slot: u16 },
    /// Progress of the clock (state word `time_slot`, milliseconds) from the instant in state word
    /// `start_slot` over `dur_ms`, as Q8 clamped to 0 ..= 256.
    TimeProg { dst: Reg, time_slot: u16, start_slot: u16, dur_ms: u32 },
    /// A critically damped spring from `from` to `to` at Q8 progress `t` (256: at rest on `to`).
    SpringEval { dst: Reg, from: Reg, to: Reg, t: Reg },
    /// 1 while the pointer is over hover site `site`, else 0. A keyed dependency: the expression is visited
    /// when its own site is entered or left, not when any site is.
    Hovered { dst: Reg, site: u32 },
}

#[derive(Clone, Copy, Debug)]
struct Head {
    start: u32,
    len: u32,
    out: Reg,
    deps: DepMask,
}

/// Every expression of a world, end to end.
#[derive(Clone, Debug, Default)]
pub struct ExprTape {
    ops: Vec<ExprOp>,
    heads: Vec<Head>,
}

/// The themes' colours: `per_theme` words per theme.
#[derive(Clone, Debug, Default)]
pub struct Palette {
    pub per_theme: u32,
    pub colors: Vec<u32>,
}
impl Palette {
    #[inline]
    pub fn get(&self, theme: u32, slot: u16) -> u32 {
        if self.per_theme == 0 {
            return 0;
        }
        let themes = self.colors.len() as u32 / self.per_theme;
        let t = if themes == 0 { 0 } else { theme.min(themes - 1) };
        self.colors.get((t * self.per_theme + slot as u32) as usize).copied().unwrap_or(0)
    }
    pub fn themes(&self) -> u32 {
        if self.per_theme == 0 { 0 } else { self.colors.len() as u32 / self.per_theme }
    }
    /// Whether colour `slot` is opaque in every theme.
    pub fn always_opaque(&self, slot: u16) -> bool {
        self.themes() > 0 && (0..self.themes()).all(|t| self.get(t, slot) >> 24 == 255)
    }
}

/// What evaluation reads.
#[derive(Clone, Copy)]
pub struct EvalCx<'a> {
    pub state: &'a StateBlock,
    pub tape: &'a ExprTape,
    pub palette: &'a Palette,
    /// The animations' current values.
    pub anims: &'a [u32],
    /// The state word of each hover depth.
    pub hover: &'a [SlotId; HOVER_DEPTHS],
}

#[inline]
fn lerp_ch(a: u32, b: u32, t: i64) -> u32 {
    (a as i64 + ((b as i64 - a as i64) * t) / 256) as u32 & 0xff
}
#[inline]
pub fn lerp_color(a: u32, b: u32, t: u32) -> u32 {
    let t = (t as i32).clamp(0, 256) as i64;
    lerp_ch(a & 0xff, b & 0xff, t) | lerp_ch(a >> 8 & 0xff, b >> 8 & 0xff, t) << 8 | lerp_ch(a >> 16 & 0xff, b >> 16 & 0xff, t) << 16 | lerp_ch(a >> 24, b >> 24, t) << 24
}
#[inline]
pub fn lerp_i32(a: u32, b: u32, t: u32) -> u32 {
    let t = (t as i32).clamp(0, 256) as i64;
    (a as i32 as i64 + ((b as i32 as i64 - a as i32 as i64) * t) / 256) as i32 as u32
}
/// The Q8 position of a critically damped spring at Q8 progress `t`: 0 at 0, 256 from `t >= 256` on.
#[inline]
pub fn spring_q8(t: u32) -> u32 {
    let t = (t as i32).clamp(0, 256);
    if t >= 256 {
        return 256;
    }
    // x(t) = 1 - (1 + w t) e^(-w t), w = 8, sampled in 257 steps
    let x = t as f32 / 256.0 * 8.0;
    let y = 1.0 - (1.0 + x) * (-x).exp();
    ((y * 256.0) as u32).min(256)
}

impl ExprTape {
    /// Begin an expression.
    pub fn build(&mut self) -> ExprBuilder<'_> {
        let start = self.ops.len() as u32;
        ExprBuilder { tape: self, start, next: 0, deps: DepMask::EMPTY }
    }
    pub fn len(&self) -> usize {
        self.heads.len()
    }
    pub fn is_empty(&self) -> bool {
        self.heads.is_empty()
    }
    pub fn ops(&self, e: u32) -> &[ExprOp] {
        let h = &self.heads[e as usize];
        &self.ops[h.start as usize..(h.start + h.len) as usize]
    }
    /// The folded set of state words expression `e` reads.
    pub fn dep_mask(&self, e: u32) -> DepMask {
        self.heads[e as usize].deps
    }
    /// Evaluate expression `e`. Registers live in this frame: nothing is allocated.
    pub fn eval(&self, e: u32, cx: &EvalCx<'_>) -> u32 {
        let Some(h) = self.heads.get(e as usize) else {
            return 0;
        };
        let mut r = [0u32; REGS];
        let g = |r: &[u32; REGS], k: Reg| r[k as usize & (REGS - 1)];
        for op in &self.ops[h.start as usize..(h.start + h.len) as usize] {
            let (dst, v) = match *op {
                ExprOp::LoadSlot { dst, slot } => (dst, cx.state.word(SlotId(slot))),
                ExprOp::Const { dst, v } => (dst, v),
                ExprOp::Add { dst, a, b } => (dst, g(&r, a).wrapping_add(g(&r, b))),
                ExprOp::Sub { dst, a, b } => (dst, g(&r, a).wrapping_sub(g(&r, b))),
                ExprOp::MulQ8 { dst, a, b } => (dst, ((g(&r, a) as i32 as i64 * g(&r, b) as i32 as i64) >> 8) as i32 as u32),
                ExprOp::DivQ8 { dst, a, b } => {
                    let d = g(&r, b) as i32 as i64;
                    (dst, if d == 0 { 0 } else { (((g(&r, a) as i32 as i64) << 8) / d) as i32 as u32 })
                }
                ExprOp::Clamp { dst, v, lo, hi } => {
                    let (lo, hi) = (g(&r, lo) as i32, g(&r, hi) as i32);
                    (dst, (g(&r, v) as i32).max(lo).min(hi.max(lo)) as u32)
                }
                ExprOp::Eq { dst, a, b } => (dst, (g(&r, a) == g(&r, b)) as u32),
                ExprOp::Select { dst, c, a, b } => (dst, if g(&r, c) != 0 { g(&r, a) } else { g(&r, b) }),
                ExprOp::LerpColor { dst, a, b, t } => (dst, lerp_color(g(&r, a), g(&r, b), g(&r, t))),
                ExprOp::LerpI32 { dst, a, b, t } => (dst, lerp_i32(g(&r, a), g(&r, b), g(&r, t))),
                ExprOp::LinearMap { dst, v, in0, in1, out0, out1 } => {
                    let (i0, i1) = (g(&r, in0) as i32 as i64, g(&r, in1) as i32 as i64);
                    let (o0, o1) = (g(&r, out0) as i32 as i64, g(&r, out1) as i32 as i64);
                    let x = (g(&r, v) as i32 as i64).clamp(i0.min(i1), i0.max(i1));
                    (dst, if i1 == i0 { o0 as i32 as u32 } else { (o0 + (x - i0) * (o1 - o0) / (i1 - i0)) as i32 as u32 })
                }
                ExprOp::PaletteLookup { dst, slot, theme_slot } => (dst, cx.palette.get(cx.state.word(SlotId(theme_slot)), slot)),
                ExprOp::TimeProg { dst, time_slot, start_slot, dur_ms } => {
                    let (now, start) = (cx.state.word(SlotId(time_slot)) as u64, cx.state.word(SlotId(start_slot)) as u64);
                    let el = now.saturating_sub(start);
                    (dst, if dur_ms == 0 || el >= dur_ms as u64 { 256 } else { (el * 256 / dur_ms as u64) as u32 })
                }
                ExprOp::SpringEval { dst, from, to, t } => (dst, lerp_i32(g(&r, from), g(&r, to), spring_q8(g(&r, t)))),
                ExprOp::Hovered { dst, site } => (dst, (cx.state.word(cx.hover[site_depth(site) & (HOVER_DEPTHS - 1)]) == site) as u32),
            };
            r[dst as usize & (REGS - 1)] = v;
        }
        r[h.out as usize & (REGS - 1)]
    }
    /// Everything expression `e` reads, exactly (the mask folds past 256 and does not hold hover sites).
    pub fn each_dep(&self, e: u32, f: &mut dyn FnMut(Dep)) {
        for op in self.ops(e) {
            match *op {
                ExprOp::LoadSlot { slot, .. } => f(Dep::Slot(SlotId(slot))),
                ExprOp::PaletteLookup { theme_slot, .. } => f(Dep::Slot(SlotId(theme_slot))),
                ExprOp::TimeProg { time_slot, start_slot, .. } => {
                    f(Dep::Slot(SlotId(time_slot)));
                    f(Dep::Slot(SlotId(start_slot)))
                }
                ExprOp::Hovered { site, .. } => f(Dep::Site(site)),
                _ => {}
            }
        }
    }
}

/// Writes one expression. Each method appends an operation and returns the register holding its result; an
/// expression that needs more than sixteen live values is refused (a panic at build time, never at evaluation).
pub struct ExprBuilder<'a> {
    tape: &'a mut ExprTape,
    start: u32,
    next: usize,
    deps: DepMask,
}

impl ExprBuilder<'_> {
    fn reg(&mut self) -> Reg {
        assert!(self.next < REGS, "an expression has at most {REGS} values; split it");
        self.next += 1;
        (self.next - 1) as Reg
    }
    fn push(&mut self, f: impl FnOnce(Reg) -> ExprOp) -> Reg {
        let dst = self.reg();
        self.tape.ops.push(f(dst));
        dst
    }
    pub fn slot(&mut self, s: impl Into<SlotId>) -> Reg {
        let s = s.into();
        self.deps.add(s);
        self.push(|dst| ExprOp::LoadSlot { dst, slot: s.0 })
    }
    pub fn konst(&mut self, v: impl Into<Val>) -> Reg {
        let Val::Const(v) = v.into() else { unreachable!() };
        self.push(|dst| ExprOp::Const { dst, v })
    }
    pub fn add(&mut self, a: Reg, b: Reg) -> Reg {
        self.push(|dst| ExprOp::Add { dst, a, b })
    }
    pub fn sub(&mut self, a: Reg, b: Reg) -> Reg {
        self.push(|dst| ExprOp::Sub { dst, a, b })
    }
    pub fn mul_q8(&mut self, a: Reg, b: Reg) -> Reg {
        self.push(|dst| ExprOp::MulQ8 { dst, a, b })
    }
    pub fn div_q8(&mut self, a: Reg, b: Reg) -> Reg {
        self.push(|dst| ExprOp::DivQ8 { dst, a, b })
    }
    pub fn clamp(&mut self, v: Reg, lo: Reg, hi: Reg) -> Reg {
        self.push(|dst| ExprOp::Clamp { dst, v, lo, hi })
    }
    pub fn eq(&mut self, a: Reg, b: Reg) -> Reg {
        self.push(|dst| ExprOp::Eq { dst, a, b })
    }
    pub fn select(&mut self, c: Reg, a: Reg, b: Reg) -> Reg {
        self.push(|dst| ExprOp::Select { dst, c, a, b })
    }
    pub fn lerp_color(&mut self, a: Reg, b: Reg, t: Reg) -> Reg {
        self.push(|dst| ExprOp::LerpColor { dst, a, b, t })
    }
    pub fn lerp_i32(&mut self, a: Reg, b: Reg, t: Reg) -> Reg {
        self.push(|dst| ExprOp::LerpI32 { dst, a, b, t })
    }
    pub fn linear_map(&mut self, v: Reg, in0: Reg, in1: Reg, out0: Reg, out1: Reg) -> Reg {
        self.push(|dst| ExprOp::LinearMap { dst, v, in0, in1, out0, out1 })
    }
    pub fn palette(&mut self, slot: u16, theme_slot: impl Into<SlotId>) -> Reg {
        let t = theme_slot.into();
        self.deps.add(t);
        self.push(|dst| ExprOp::PaletteLookup { dst, slot, theme_slot: t.0 })
    }
    pub fn time_prog(&mut self, time_slot: impl Into<SlotId>, start_slot: impl Into<SlotId>, dur_ms: u32) -> Reg {
        let (t, s) = (time_slot.into(), start_slot.into());
        self.deps.add(t);
        self.deps.add(s);
        self.push(|dst| ExprOp::TimeProg { dst, time_slot: t.0, start_slot: s.0, dur_ms })
    }
    /// 1 while hover site `site` is under the pointer.
    pub fn hovered(&mut self, site: u32) -> Reg {
        self.push(|dst| ExprOp::Hovered { dst, site })
    }
    pub fn spring(&mut self, from: Reg, to: Reg, t: Reg) -> Reg {
        self.push(|dst| ExprOp::SpringEval { dst, from, to, t })
    }
    /// End the expression: its value is register `out`.
    pub fn finish(self, out: Reg) -> Val {
        let len = self.tape.ops.len() as u32 - self.start;
        self.tape.heads.push(Head { start: self.start, len, out, deps: self.deps });
        Val::Expr(self.tape.heads.len() as u32 - 1)
    }
}

impl<T> From<super::state::Slot<T>> for SlotId {
    fn from(s: super::state::Slot<T>) -> SlotId {
        s.id()
    }
}

/// Evaluate a value. `Anim` reads the animation's current value; `Palette` is looked up now (the lazy form on a
/// fill is the store's business, not evaluation's).
#[inline]
pub fn eval(v: Val, cx: &EvalCx<'_>) -> u32 {
    match v {
        Val::Const(c) => c,
        Val::Slot(s) => cx.state.word(SlotId(s)),
        Val::Palette { slot, theme_slot } => cx.palette.get(cx.state.word(SlotId(theme_slot)), slot),
        Val::HoverSel { site, off, on } => {
            if cx.state.word(cx.hover[site_depth(site) & (HOVER_DEPTHS - 1)]) == site { on } else { off }
        }
        Val::HoverPal { site, slot, theme_slot, on } => {
            if cx.state.word(cx.hover[site_depth(site) & (HOVER_DEPTHS - 1)]) == site { on } else { cx.palette.get(cx.state.word(SlotId(theme_slot)), slot) }
        }
        Val::Anim(a) => cx.anims.get(a as usize).copied().unwrap_or(0),
        Val::Expr(e) => cx.tape.eval(e, cx),
    }
}

/// What a value's result depends on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dep {
    Slot(SlotId),
    Site(u32),
    Anim(u32),
}

/// Every dependency of a value, exactly.
pub fn each_dep(v: Val, tape: &ExprTape, f: &mut dyn FnMut(Dep)) {
    match v {
        Val::Const(_) => {}
        Val::Slot(s) => f(Dep::Slot(SlotId(s))),
        Val::Palette { theme_slot, .. } => f(Dep::Slot(SlotId(theme_slot))),
        Val::HoverSel { site, .. } => f(Dep::Site(site)),
        Val::HoverPal { site, theme_slot, .. } => {
            f(Dep::Site(site));
            f(Dep::Slot(SlotId(theme_slot)))
        }
        Val::Anim(a) => f(Dep::Anim(a)),
        Val::Expr(e) => tape.each_dep(e, f),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cx<'a>(state: &'a StateBlock, tape: &'a ExprTape, palette: &'a Palette, hover: &'a [SlotId; HOVER_DEPTHS]) -> EvalCx<'a> {
        EvalCx { state, tape, palette, anims: &[], hover }
    }

    #[test]
    fn an_expression_is_a_run_of_register_operations() {
        let mut st = StateBlock::new();
        let hover = [SlotId(0); HOVER_DEPTHS];
        let n = st.alloc(5i32);
        let mut tape = ExprTape::default();
        // clamp(n * 3 - 4, 0, 10)
        let mut b = tape.build();
        let (x, three, four, zero, ten) = (b.slot(n), b.konst(3 << 8), b.konst(4), b.konst(0), b.konst(10));
        let m = b.mul_q8(x, three);
        let s = b.sub(m, four);
        let c = b.clamp(s, zero, ten);
        let e = b.finish(c);
        let Val::Expr(ei) = e else { panic!() };
        let pal = Palette::default();
        assert_eq!(eval(e, &cx(&st, &tape, &pal, &hover)), 10);
        st.set(n, 2);
        assert_eq!(eval(e, &cx(&st, &tape, &pal, &hover)), 2);
        st.set(n, -7);
        assert_eq!(eval(e, &cx(&st, &tape, &pal, &hover)), 0);
        let mut deps = Vec::new();
        each_dep(e, &tape, &mut |d| deps.push(d));
        assert_eq!(deps, vec![Dep::Slot(n.id())]);
        assert!(tape.dep_mask(ei).has(n.id()) && !tape.dep_mask(ei).has(SlotId(9)));
    }

    #[test]
    fn select_lerp_map_palette_time_and_spring() {
        let mut st = StateBlock::new();
        let hover = [SlotId(0); HOVER_DEPTHS];
        let (sel, theme, time, start) = (st.alloc(1u32), st.alloc(0u32), st.alloc(0u32), st.alloc(100u32));
        let pal = Palette { per_theme: 2, colors: vec![0xff00_0000, 0xff11_1111, 0xffff_ffff, 0xffee_eeee] };
        let mut tape = ExprTape::default();
        let mut b = tape.build();
        let (s, one) = (b.slot(sel), b.konst(1));
        let is = b.eq(s, one);
        let (bg, fg) = (b.palette(0, theme), b.palette(1, theme));
        let out = b.select(is, fg, bg);
        let picked = b.finish(out);
        assert_eq!(eval(picked, &cx(&st, &tape, &pal, &hover)), 0xff11_1111);
        st.set(theme, 1);
        assert_eq!(eval(picked, &cx(&st, &tape, &pal, &hover)), 0xffee_eeee);
        st.set(sel, 0);
        assert_eq!(eval(picked, &cx(&st, &tape, &pal, &hover)), 0xffff_ffff);

        let mut b = tape.build();
        let t = b.time_prog(time, start, 200);
        let (a, z) = (b.konst(0xff00_0000u32), b.konst(0xff00_00ffu32));
        let c = b.lerp_color(a, z, t);
        let fade = b.finish(c);
        let mut b = tape.build();
        let t = b.time_prog(time, start, 200);
        let (from, to) = (b.konst(10), b.konst(110));
        let sp = b.spring(from, to, t);
        let spring = b.finish(sp);
        let mut b = tape.build();
        let (v, i0, i1, o0, o1) = (b.slot(time), b.konst(100), b.konst(300), b.konst(-50), b.konst(50));
        let m = b.linear_map(v, i0, i1, o0, o1);
        let map = b.finish(m);
        let at = |st: &StateBlock, v| eval(v, &cx(st, &tape, &pal, &hover));
        assert_eq!((at(&st, fade), at(&st, spring) as i32, at(&st, map) as i32), (0xff00_0000, 10, -50));
        st.set(time, 200);
        assert_eq!(at(&st, fade), 0xff00_007f);
        let mid = at(&st, spring) as i32;
        assert!(mid > 90 && mid < 110, "a critically damped spring is most of the way at half time: {mid}");
        assert_eq!(at(&st, map) as i32, 0);
        st.set(time, 5000);
        assert_eq!((at(&st, fade), at(&st, spring) as i32, at(&st, map) as i32), (0xff00_00ff, 110, 50));
    }

    #[test]
    fn a_hover_select_is_one_compare_against_its_depths_word() {
        let mut st = StateBlock::new();
        let mut hover = [SlotId(0); HOVER_DEPTHS];
        for h in hover.iter_mut() {
            *h = st.alloc(NO_SITE).id();
        }
        let (tape, pal) = (ExprTape::default(), Palette::default());
        let row = 1 << 24 | 17;
        let v = Val::HoverSel { site: row, off: 1, on: 2 };
        assert_eq!(eval(v, &cx(&st, &tape, &pal, &hover)), 1);
        st.set_word(hover[1], row);
        assert_eq!(eval(v, &cx(&st, &tape, &pal, &hover)), 2);
        st.set_word(hover[1], 1 << 24 | 18);
        assert_eq!(eval(v, &cx(&st, &tape, &pal, &hover)), 1);
    }

    #[test]
    #[should_panic(expected = "at most 16 values")]
    fn a_seventeenth_value_is_refused_when_built() {
        let mut tape = ExprTape::default();
        let mut b = tape.build();
        for k in 0..17 {
            b.konst(k);
        }
    }
}
