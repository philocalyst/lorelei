//! The walk: a description written as [`El`](super::El)s is mounted, re-described and lowered through the
//! kernel's own builders (`LBox`, `Para`, the act table, `describe::mount_dyn`), with the operands erased to
//! a value and an optional wire request. Pixels, damage, wires and records are therefore the kernel's.
//!
//! Identity (L3, parent × ordinal × kind): the children of every box are one dynamic part whose items are keyed
//! by their own shape (kind, paint, text metrics, reactive operations, wire and route identity) and never
//! sealed; an item whose key changes is remounted at its position, and only it. `Option`, `Vec` and iterator
//! children are dynamic parts that are sealed, so an item described exactly as before is stepped over without
//! being entered, and a shift by one is recognised (`describe::update_dyn`).
use super::arena::*;
use crate::boxops::{ApplyBoxH, ApplyBoxW, ApplyOpacity, ApplyTx, ApplyTy, BoxLow, BoxOps};
use crate::color::Color;
use crate::describe::{mount_dyn_as, update_dyn, update_opaque};
use crate::elem::{Apply, Element, IntoProp, Mounter, TextSrc, decimal};
use crate::input::{Ev, Key, Route};
use crate::layout::{Dim, LBox, Para, Spec};
use crate::rt::*;
use crate::seal::Sealer;
use bough_core::bough::hit::{KF_FOCUS, KF_INTERACT};
use std::any::TypeId;
use std::ptr::{self, NonNull};

// =============================================================================== lazy port values
//
// A port with a wire request (`.bg(&self.tint)`, `.opacity(self.n.map(f))`, `text(self.r.map(f))`, ...) carries no
// value in its node: building the description runs no closure and reads no cell (`plug.rs`). Its value is the
// request's `eval` of the cell and the recipe, computed only where it is needed:
//
// * at mount, for the record's first value (`EOp::lower`, `EText::lower`);
// * at an update, when the port's wire is not installed (its cell is not a field of the owner, so the
//   description is the only way a new value reaches the record), or when it is installed but the recipe
//   changed (a map whose captures changed: the installed wire takes the new recipe and the value is applied on
//   this turn's frame) (`resolve_ports`);
// * when lowering (`Item::lower`), and in a seal, for a port whose wire is not installed.
//
// A port whose wire is installed is described by its identity alone (cell, patch, recipe): its value is the
// wire's to keep current, so neither its seal nor its update looks at it, and its builder neither computes nor
// applies it (`P_KEEP`, `IntoProp::keep`).
//
// Where field-ness is decided. A wire is installed exactly when `elem::field_offset::<V>(owner, cell, ty)` says
// the cell is a field of the mounting owner (`Mounter::wire_field`). That is a pure function of the owner's
// address and type and the cell's address and type. The seal asks the same function (`installed`, through
// `WalkOwner`, which every walk sets from its `Mounter` before it seals or updates anything: the root `El`'s
// `Element` impl), and writes which it found into the seal (`WIRED` or `CONST` as the port's first word). A
// retained seal is only ever checked by updates of the owner that wrote it: a dynamic part belongs to its
// owner's tape, and an owner does not move while mounted (the runtime's wires hold the absolute addresses of
// its cells' heads). So a retained `WIRED` port whose identity (cell address, patch, recipe) matches the
// description's is still a field, and the fast check trusts the word instead of asking again; a `CONST` port
// compares its evaluated value. The update asks the runtime directly: the item's scope holds the wires its
// mount installed, and a port is kept only if one of them is its own (`find_wire`), so a port that was not
// installed is never kept, whatever the seal said.

/// The seal's first word of a port whose wire is installed (its value is not part of the seal).
const WIRED: u32 = 0x5749_5245;
/// The seal's first word of a port whose cell is not a field: a constant of this description, its value follows.
const CONST: u32 = 0x434e_5354;

/// The owner a walk describes for: what `installed` asks the field rule about.
#[derive(Clone, Copy)]
struct WalkOwner {
    base: usize,
    /// `elem::field_offset::<V>(base, at, ty).is_some()` for the owner's type `V`.
    field: fn(usize, usize, TypeId) -> bool,
}
fn no_field(_: usize, _: usize, _: TypeId) -> bool {
    false
}
fn field_of_owner<V>(base: usize, at: usize, ty: TypeId) -> bool {
    crate::elem::field_offset::<V>(base, at, ty).is_some()
}
thread_local! {
    /// The owner of the walk in progress (none: nothing is a field, as for a description sealed outside a walk).
    static OWNER: std::cell::Cell<WalkOwner> = const { std::cell::Cell::new(WalkOwner { base: 0, field: no_field }) };
}
/// Sets the walk's owner for its lifetime (walks nest: an entity's view inside a view restores the outer one).
pub(crate) struct OwnerScope(WalkOwner);
impl OwnerScope {
    #[inline]
    pub(crate) fn of<V>(m: &Mounter<'_, V>) -> OwnerScope {
        OwnerScope(OWNER.replace(WalkOwner { base: m.owner as usize, field: field_of_owner::<V> }))
    }
    /// `owner` as the walk's owner (tests: a seal written outside a mount).
    #[cfg(test)]
    fn raw<V>(owner: &V) -> OwnerScope {
        OwnerScope(OWNER.replace(WalkOwner { base: owner as *const V as usize, field: field_of_owner::<V> }))
    }
}
impl Drop for OwnerScope {
    #[inline]
    fn drop(&mut self) {
        OWNER.set(self.0);
    }
}

/// Is `w`'s cell a field of the walk's owner (so its wire is, or will be, installed)?
#[inline]
#[allow(dead_code, reason = "the fast check trusts the retained field verdict; the eager mutant uses this to write constants")]
fn installed(w: &WireReq) -> bool {
    // (mutant: every port is a constant of its description: its value enters the seal)
    if cfg!(feature = "mutant_el_eager_port") {
        return false;
    }
    let o = OWNER.get();
    (o.field)(o.base, w.cell(), w.ty)
}

/// The installed wire of the item being updated that `req` asked for: same head, same patch, the first not
/// matched yet (`used`: the item's wires already matched, by their place in its scope).
#[inline]
fn find_wire<V>(m: &Mounter<'_, V>, used: &mut u64, req: &WireReq) -> Option<u32> {
    let head = req.wire_head() as usize;
    for (j, &wi) in m.scope.wires.iter().enumerate().take(64) {
        if *used & 1 << j != 0 {
            continue;
        }
        let w = &m.rt.wires[wi as usize];
        if !w.dead && w.head == head && w.patch as usize == req.patch as usize {
            *used |= 1 << j;
            return Some(wi);
        }
    }
    None
}

/// Before a node's builder updates it: each of its ports is kept (`P_KEEP`: installed, same recipe) or
/// evaluated where the builder reads it. An installed port whose recipe changed takes the description's recipe
/// (a capture changed: the wire computes with it from now on) and is evaluated and applied now.
#[inline(never)]
fn resolve_ports<V>(m: &mut Mounter<'_, V>, n: &DNode) {
    let mut used = 0u64;
    let mut one = |m: &mut Mounter<'_, V>, w: *const WireReq| {
        // SAFETY: a request of this description; the walk holds no other reference to it.
        let Some(w) = (unsafe { (w as *mut WireReq).as_mut() }) else { return };
        // No match (and every port under the eager mutant) is a constant of this description: mark it so the
        // fast check knows to compare the evaluated value, as the streaming seal wrote it.
        w.state = P_CONST;
        if let Some(wi) = find_wire(m, &mut used, w) {
            let iw = &mut m.rt.wires[wi as usize];
            if iw.recipe.bytes() == w.recipe.bytes() {
                w.state = P_KEEP;
            } else {
                iw.recipe = w.recipe;
            }
        }
    };
    // in the order the mount installed them (a box: fill, then opacity, translations, width, height; a text:
    // content, then ink), so two ports on one cell through one patch match their own wires
    match n.kind {
        K_BOX => {
            one(m, n.ink);
            if let Some(x) = ext(n) {
                for (bit, w) in [(OP_ALPHA, x.wa), (OP_TX, x.wtx), (OP_TY, x.wty), (OP_W, x.ww), (OP_H, x.wh)] {
                    if n.ops & bit != 0 {
                        one(m, w);
                    }
                }
            }
        }
        K_TEXT => {
            one(m, n.tw);
            one(m, n.ink);
        }
        _ => {}
    }
}

// =============================================================================== erased operands

/// A port's operand as the walk hands it to a kernel builder: the described value (a constant), or the wire
/// request whose eval gives it (a port: "Lazy port values").
pub(crate) struct EOp<P> {
    v: P,
    w: *const WireReq,
}

/// Installs `w` on `node` if its cell is a field of the mounting view.
#[inline]
fn wire<V>(m: &mut Mounter<'_, V>, w: *const WireReq, node: u32) {
    if !w.is_null() {
        // SAFETY: `w` was written this description by a setter: `head` is a head `sub()` bytes into a live cell
        // of the type `ty` names, and `patch` reads that cell type with this recipe (`plug.rs`).
        unsafe {
            let w = &*w;
            if m.wire_field(&*w.wire_head(), w.sub() as u32, w.ty, w.patch, node, w.recipe) {
                if w.tag() == W_LATEST {
                    let i = *m.scope.wires.last().unwrap();
                    m.rt.make_latest(i);
                }
                if w.tag() == W_PHEAD {
                    // the pending flag was read when `view` ran, and a producer mounted since (its first request)
                    // may have moved it: the port is settled once more from the reader's state now
                    crate::signal::queue(&*w.wire_head());
                    m.rt.asy.rescan = true;
                }
            }
        }
    }
}

/// A port's identity in a seal: `WIRED` or `CONST`, the cell's address, the patch and its recipe; a `CONST`
/// port's value after them (`paint`: a colour, sealed as paint).
#[inline(always)]
fn seal_port<S: Sealer>(s: &mut S, w: *const WireReq, paint: bool) {
    // SAFETY: as `wire`.
    let Some(w) = (unsafe { w.as_ref() }) else {
        s.word(0);
        return;
    };
    let wired = seal_ident(s, w);
    if !wired {
        let b = w.scalar();
        if paint { s.paint(b) } else { s.word(b) }
    }
}
/// `seal_port` for a text port: a `CONST` one's text after its identity.
#[inline(always)]
fn seal_text_port<S: Sealer>(s: &mut S, w: &WireReq) {
    if !seal_ident(s, w) {
        w.text(|t| s.bytes(t.as_bytes()));
    }
}
/// The identity words of a port (whether its wire is installed).
#[inline]
fn seal_ident<S: Sealer>(s: &mut S, w: &WireReq) -> bool {
    let wired = installed(w);
    s.word(if wired { WIRED } else { CONST });
    let (a, p) = (w.head as usize as u64, w.patch as usize as u64);
    s.word(a as u32);
    s.word((a >> 32) as u32);
    s.word(p as u32);
    s.word((p >> 32) as u32);
    for c in w.recipe.bytes().chunks_exact(4) {
        s.word(u32::from_ne_bytes([c[0], c[1], c[2], c[3]]));
    }
    wired
}

/// How a scalar operand's value enters a seal, and is read back from a port's eval.
pub(crate) trait SealVal: Copy + 'static {
    /// A colour (sealed as paint).
    const PAINT: bool;
    fn seal_val<S: Sealer>(self, s: &mut S);
    fn from_bits(b: u32) -> Self;
}
impl SealVal for Color {
    const PAINT: bool = true;
    #[inline(always)]
    fn seal_val<S: Sealer>(self, s: &mut S) {
        s.paint(self.0)
    }
    #[inline(always)]
    fn from_bits(b: u32) -> Self {
        Color(b)
    }
}
impl SealVal for f32 {
    const PAINT: bool = false;
    #[inline(always)]
    fn seal_val<S: Sealer>(self, s: &mut S) {
        s.f32(self)
    }
    #[inline(always)]
    fn from_bits(b: u32) -> Self {
        f32::from_bits(b)
    }
}

impl<P: SealVal> EOp<P> {
    /// The operand's value now: the constant, or the port's eval.
    #[inline]
    fn now(&self) -> P {
        // SAFETY: as `wire`.
        match unsafe { self.w.as_ref() } {
            None => self.v,
            Some(w) => P::from_bits(w.scalar()),
        }
    }
    #[inline(always)]
    fn kept(&self) -> bool {
        // SAFETY: as `wire`.
        unsafe { self.w.as_ref() }.is_some_and(WireReq::keep)
    }
}

impl<P: SealVal, V> IntoProp<P, V> for EOp<P> {
    #[inline]
    fn lower<K: Apply<P = P>>(self, m: &mut Mounter<'_, V>, node: u32) -> P {
        wire(m, self.w, node);
        self.now()
    }
    #[inline(always)]
    fn value(self) -> P {
        self.now()
    }
    #[inline(always)]
    fn keep(&self) -> bool {
        self.kept()
    }
    #[inline(always)]
    fn seal<S: Sealer>(&self, s: &mut S) {
        // the node's value (a constant's; a port's placeholder), then the port: a port that is not a field seals
        // its value there (its wire is not installed, so the value reaches the record only through a
        // re-description)
        self.v.seal_val(s);
        seal_port(s, self.w, P::PAINT);
    }
}

/// A text operand: borrowed or copied bytes, or an integer (a constant), or a port's request.
pub(crate) struct EText {
    ptr: *const u8,
    len: u64,
    flags: u8,
    w: *const WireReq,
}

impl EText {
    #[inline(always)]
    fn with<R>(&self, f: impl FnOnce(&str) -> R) -> R {
        // SAFETY: as `wire`.
        if let Some(w) = unsafe { self.w.as_ref() } {
            return w.text(f);
        }
        if self.flags & F_INT != 0 {
            let v = if self.flags & F_NEG != 0 { self.len as i64 as i128 } else { self.len as i128 };
            let mut buf = [0u8; 40];
            f(decimal(v, &mut buf))
        } else {
            // SAFETY: `ptr`/`len` are UTF-8 bytes borrowed for the description's lifetime or copied into its
            // arena (`plug.rs`); the walk runs inside that lifetime.
            f(unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(self.ptr, self.len as usize)) })
        }
    }
}

impl<V> TextSrc<V> for EText {
    #[inline]
    fn lower(self, m: &mut Mounter<'_, V>, node: u32) -> String {
        wire(m, self.w, node);
        self.with(str::to_owned)
    }
    #[inline]
    fn value(self) -> String {
        self.with(str::to_owned)
    }
    #[inline(always)]
    fn keep(&self) -> bool {
        // SAFETY: as `wire`.
        unsafe { self.w.as_ref() }.is_some_and(WireReq::keep)
    }
    #[inline]
    fn with_str<R>(self, f: impl FnOnce(&str) -> R) -> R {
        self.with(f)
    }
    #[inline(always)]
    fn seal<S: Sealer>(&self, s: &mut S) {
        self.seal_text(s)
    }
}

impl EText {
    #[inline(always)]
    fn seal_text<S: Sealer>(&self, s: &mut S) {
        // the node's text (a port's: the empty placeholder), then a port's identity and a constant port's text
        if self.flags & F_INT != 0 {
            s.int(self.len);
        } else {
            // SAFETY: as `with`.
            s.bytes(unsafe { std::slice::from_raw_parts(self.ptr, self.len as usize) });
        }
        // SAFETY: as `wire`.
        if let Some(w) = unsafe { self.w.as_ref() } {
            seal_text_port(s, w);
        }
    }
}

/// A box's reactive operations (`boxops.rs`), whichever of them the description set.
pub(crate) struct EOps {
    x: *const Ext,
    ops: u8,
}

impl EOps {
    #[inline(always)]
    fn x(&self) -> &Ext {
        // SAFETY: `ops != 0` only when the node has an `Ext` (the setters that set an op allocate it).
        unsafe { &*self.x }
    }
    #[inline(always)]
    fn op(&self, w: *const WireReq, v: f32) -> EOp<f32> {
        EOp { v, w }
    }
}

impl<V> BoxOps<V> for EOps {
    const SIZE: bool = true;
    const PAINT: bool = true;
    #[inline(always)]
    fn has_size(&self) -> bool {
        self.ops & (OP_W | OP_H) != 0
    }
    #[inline(always)]
    fn has_paint(&self) -> bool {
        self.ops & (OP_ALPHA | OP_TX | OP_TY) != 0
    }
    #[inline(always)]
    fn owns_w(&self) -> bool {
        self.ops & OP_W != 0
    }
    #[inline(always)]
    fn owns_h(&self) -> bool {
        self.ops & OP_H != 0
    }
    fn install(self, m: &mut Mounter<'_, V>, lid: u32, node: u32, spec: &mut Spec) {
        if self.ops == 0 {
            return;
        }
        let x = self.x();
        if self.ops & OP_ALPHA != 0 {
            let v = self.op(x.wa, x.alpha).lower::<ApplyOpacity>(m, node);
            ApplyOpacity::apply(m.rt, node, &v);
        }
        if self.ops & (OP_TX | OP_TY) != 0 {
            let s = m.rt.scale;
            let tx = if self.ops & OP_TX != 0 { self.op(x.wtx, x.tx).lower::<ApplyTx>(m, node) } else { 0.0 };
            let ty = if self.ops & OP_TY != 0 { self.op(x.wty, x.ty).lower::<ApplyTy>(m, node) } else { 0.0 };
            let n = m.rt.mod_of(node);
            (n.tx, n.ty) = (tx * s, ty * s);
        }
        if self.ops & OP_W != 0 {
            spec.w = Dim::Pt(self.op(x.ww, x.w).lower::<ApplyBoxW>(m, lid));
        }
        if self.ops & OP_H != 0 {
            spec.h = Dim::Pt(self.op(x.wh, x.h).lower::<ApplyBoxH>(m, lid));
        }
    }
    fn refresh(self, rt: &mut Rt, lid: u32, node: u32) {
        if self.ops == 0 {
            return;
        }
        let x = self.x();
        // a kept port (its wire installed and current) is neither evaluated nor applied
        let now = |bit: u8, w: *const WireReq, v: f32| (self.ops & bit != 0).then(|| self.op(w, v)).filter(|o| !o.kept()).map(|o| o.now());
        if let Some(v) = now(OP_ALPHA, x.wa, x.alpha) {
            ApplyOpacity::apply(rt, node, &v);
        }
        if let Some(v) = now(OP_TX, x.wtx, x.tx) {
            ApplyTx::apply(rt, node, &v);
        }
        if let Some(v) = now(OP_TY, x.wty, x.ty) {
            ApplyTy::apply(rt, node, &v);
        }
        if let Some(v) = now(OP_W, x.ww, x.w) {
            ApplyBoxW::apply(rt, lid, &v);
        }
        if let Some(v) = now(OP_H, x.wh, x.h) {
            ApplyBoxH::apply(rt, lid, &v);
        }
    }
    fn lower_ops(self, spec: &mut Spec, o: &mut BoxLow) {
        if self.ops == 0 {
            return;
        }
        let x = self.x();
        if self.ops & OP_ALPHA != 0 {
            o.alpha = self.op(x.wa, x.alpha).now();
        }
        if self.ops & OP_TX != 0 {
            o.shift.0 = self.op(x.wtx, x.tx).now();
        }
        if self.ops & OP_TY != 0 {
            o.shift.1 = self.op(x.wty, x.ty).now();
        }
        if self.ops & OP_W != 0 {
            spec.w = Dim::Pt(self.op(x.ww, x.w).now());
        }
        if self.ops & OP_H != 0 {
            spec.h = Dim::Pt(self.op(x.wh, x.h).now());
        }
    }
    #[inline(always)]
    fn seal<S: Sealer>(&self, s: &mut S) {
        s.word(self.ops as u32);
        self.seal_ops(s)
    }
}

impl EOps {
    #[inline(always)]
    fn seal_ops<S: Sealer>(&self, s: &mut S) {
        if self.ops == 0 {
            return;
        }
        let x = self.x();
        for (bit, v, w) in [(OP_ALPHA, x.alpha, x.wa), (OP_TX, x.tx, x.wtx), (OP_TY, x.ty, x.wty), (OP_W, x.w, x.ww), (OP_H, x.h, x.wh)] {
            if self.ops & bit != 0 {
                s.f32(v);
                seal_port(s, w, false);
            }
        }
    }
}

// =============================================================================== nodes as kernel builders

/// One node of the description: an item of a dynamic part.
#[derive(Clone, Copy)]
pub(crate) struct Item(pub(crate) NonNull<DNode>);

impl Item {
    #[inline(always)]
    fn n(&self) -> &DNode {
        // SAFETY: an `Item` is made only by the walk from nodes of the description being walked; the arena is not
        // rewound while a walk (`Walk`) is in progress.
        unsafe { self.0.as_ref() }
    }
}

/// The children of a node, in order.
pub(crate) struct Items(*const DNode);
impl Iterator for Items {
    type Item = Item;
    #[inline(always)]
    fn next(&mut self) -> Option<Item> {
        let n = NonNull::new(self.0 as *mut DNode)?;
        // SAFETY: as `Item::n`.
        self.0 = unsafe { n.as_ref().next };
        Some(Item(n))
    }
}

/// A box's children: one dynamic part, never sealed (an item is keyed by its own shape).
pub(crate) struct EKids(*const DNode);

impl<V: 'static> Element<V> for EKids {
    #[inline]
    fn mount(self, m: &mut Mounter<'_, V>) {
        mount_dyn_as(m, Items(self.0), false);
    }
    #[inline]
    fn update(self, m: &mut Mounter<'_, V>) {
        update_dyn(m, Items(self.0));
    }
    #[inline]
    fn seal<S: Sealer>(&self, s: &mut S) {
        let mut k = 0;
        for c in Items(self.0) {
            seal_node(c.n(), s);
            k += 1;
        }
        s.word(0x4b49_0000 | k);
    }
    fn lower(self, l: &mut crate::lower::Lower<'_, '_, V>) {
        for c in Items(self.0) {
            c.lower(l);
        }
    }
}

#[inline(always)]
fn lbox(n: &DNode) -> LBox<EOp<Color>, EKids, EOps> {
    LBox { spec: n.spec, draw: n.flags & F_DRAW != 0, bg: EOp { v: Color(n.paint), w: n.ink }, radius: n.m[0], border: if n.flags & F_BORDER != 0 { Some((n.m[1], Color(n.bc))) } else { None }, ch: EKids(n.first), x: EOps { x: n.ext, ops: n.ops } }
}

#[inline(always)]
fn para(n: &DNode) -> Para<EText, EOp<Color>> {
    Para { s: EText { ptr: n.text, len: n.len, flags: n.flags, w: n.tw }, spec: n.spec, size: n.m[0], lh: n.m[1], color: EOp { v: Color(n.paint), w: n.ink }, wrap: n.flags & F_WRAP != 0 }
}

#[inline(always)]
fn image(n: &DNode) -> crate::ui::ImageBox {
    // SAFETY: an image node's `text` is the `ImageBox` its builder put in the bump store.
    let mut b = unsafe { *(n.text as *const crate::ui::ImageBox) };
    b.spec = n.spec;
    b
}

/// A text input in a box carrying the node's layout (margins, flex settings), the box being what routes attach to.
#[inline(always)]
fn input_box<C>(n: &DNode, ch: C) -> LBox<EOp<Color>, C, EOps> {
    LBox { spec: n.spec, draw: false, bg: EOp { v: Color(0), w: std::ptr::null() }, radius: 0.0, border: None, ch, x: EOps { x: std::ptr::null(), ops: 0 } }
}

/// What a re-description of an opaque element walks: nothing (`describe::update_opaque` steps over it).
pub(crate) struct Opaque;
impl<V> Element<V> for Opaque {
    fn mount(self, _: &mut Mounter<'_, V>) {
        unreachable!("an opaque element is mounted by its own builder")
    }
    #[inline]
    fn update(self, m: &mut Mounter<'_, V>) {
        update_opaque(m)
    }
}

/// A text input's signal, proven a field of the mounting view.
#[track_caller]
fn input_signal<'a, V: 'static>(m: &Mounter<'_, V>, n: &DNode) -> crate::signal::Wired<'a, String, V> {
    let sig = n.text as *const crate::signal::Signal<String>;
    let (at, base) = (sig as usize, m.owner as usize);
    let off = at.wrapping_sub(base);
    let ok = at >= base && off < size_of::<V>() && off % 4 == 0 && crate::reflect::cells::<V>().has(off, crate::reflect::cell_id::<crate::signal::Signal<String>>());
    assert!(ok, "`text_input` takes a `Signal<String>` field of the view `{}` (the input writes it)", std::any::type_name::<V>());
    // SAFETY: the owner type has a `Signal<String>` field at `off` (the cell table), and the signal lives at
    // that address inside the live owner, so it is that field; it outlives the mount.
    unsafe { crate::signal::Wired::at(&*sig, off as u32) }
}

#[inline(always)]
fn prod(n: &DNode) -> &ProdReq {
    // SAFETY: a producer node's `text` is the `ProdReq` its builder put in the bump store.
    unsafe { &*(n.text as *const ProdReq) }
}
/// A producer's identity: its source, its reader, its request function.
#[inline(always)]
fn prod_words(p: &ProdReq) -> [u64; 3] {
    [p.src as usize as u64, p.dst as usize as u64, p.f as u64]
}
#[cold]
#[inline(never)]
fn seal_prod<S: Sealer>(n: &DNode, s: &mut S) {
    for w in prod_words(prod(n)) {
        s.word(w as u32);
        s.word((w >> 32) as u32);
    }
}
/// `seal_prod`'s words at `p` (then: the position after them; else null).
#[cold]
#[inline(never)]
fn check_prod(n: &DNode, p: At, end: At) -> At {
    if left(p, end) < 24 {
        return ptr::null();
    }
    let [a, b, c] = prod_words(prod(n));
    // SAFETY: 24 bytes are left (checked).
    let d = unsafe { (rd32(p, 0) ^ a as u32) | (rd32(p, 4) ^ (a >> 32) as u32) | (rd32(p, 8) ^ b as u32) | (rd32(p, 12) ^ (b >> 32) as u32) | (rd32(p, 16) ^ c as u32) | (rd32(p, 20) ^ (c >> 32) as u32) };
    if d != 0 { ptr::null() } else { unsafe { p.add(24) } }
}
#[inline(always)]
fn bound(n: &DNode) -> &BoundReq {
    // SAFETY: a boundary node's `text` is the `BoundReq` its builder put in the bump store.
    unsafe { &*(n.text as *const BoundReq) }
}
fn guards(b: &BoundReq) -> impl Iterator<Item = &GuardReq> {
    let mut g = b.guards;
    std::iter::from_fn(move || {
        // SAFETY: the guard list of a node of this description.
        let r = unsafe { g.as_ref()? };
        g = r.next;
        Some(r)
    })
}

/// A boundary's identity in its seal: its policy and guard count, then each guard reader's address.
#[inline]
fn bound_words(n: &DNode) -> impl Iterator<Item = u32> + '_ {
    let b = bound(n);
    let count = guards(b).count() as u32;
    std::iter::once(0x4247_0000 | (b.policy as u32) << 8 | count.min(255)).chain(guards(b).flat_map(|g| [g.reader as u32, (g.reader as u64 >> 32) as u32]))
}

/// The offset of a cell of type `ty` at `at` in the mounting view, if it is one of its fields (the cell table).
fn field_of<V>(m: &Mounter<'_, V>, at: usize, ty: TypeId) -> Option<u32> {
    crate::elem::field_offset::<V>(m.owner as usize, at, ty)
}

#[cold]
#[inline(never)]
fn not_fields<V>(p: &ProdReq) -> ! {
    let (s, t) = (p.names)();
    panic!("{}: `produce` takes a `Signal<{s}>` field and a `Reader<{t}>` field of the view `{}` (a producer writes its reader after the description is gone)", p.at, std::any::type_name::<V>())
}

/// A producer: both cells proven fields of the view, then the kernel's own producer mount.
fn mount_prod<V: 'static>(m: &mut Mounter<'_, V>, n: &DNode) {
    let p = prod(n);
    let (Some(src), Some(dst)) = (field_of(m, p.src as usize, p.src_ty), field_of(m, p.dst as usize, p.dst_ty)) else { not_fields::<V>(p) };
    // SAFETY: the cell table proved a `Signal<S>` at `src` and a `Reader<T>` at `dst` of the live owner `V`
    // (the types `mount` was monomorphised for, `produce`), and `f` is that producer's request function.
    unsafe { (p.mount)(m.rt, m.scope, m.owner as *const u8, m.cls, src, dst, p.f) };
}

/// A boundary: its guards that are fields of the view (wired: a reader elsewhere is a constant of the
/// description), three clips, and each arm mounted under its own as a sealed dynamic part of one item (an arm
/// described as before is stepped over; a kind change remounts it there). The tape holds the boundary's index,
/// then the arms' parts.
fn mount_bound<V: 'static>(m: &mut Mounter<'_, V>, n: &DNode) {
    let b = bound(n);
    let field: Vec<(usize, u32)> = guards(b).filter_map(|g| field_of(m, g.reader, g.ty).map(|o| (g.reader, o))).collect();
    let rect = if m.clip == 0 { m.rt.bough.window() } else { m.rt.bough.clip_rect(m.clip) };
    let (bi, clips) = m.rt.el_bound(rect, field.iter().map(|g| g.0).collect(), b.policy);
    for &(r, off) in &field {
        // SAFETY: a `Reader<_>` field at `off` (the cell table); its pending head is 4 bytes in; the patch reads
        // the boundary, not the cell.
        unsafe { m.wire_cell(&*((r + 4) as *const std::cell::Cell<u32>), off + 4, crate::reader::patch_el_bound, bi, crate::rt::Recipe::NONE) };
    }
    m.tape_push(bi);
    let prev = m.clip;
    for (i, a) in Items(n.first).enumerate() {
        m.clip = clips[i];
        mount_dyn_as(m, std::iter::once(a), true);
    }
    m.clip = prev;
}

fn update_bound<V: 'static>(m: &mut Mounter<'_, V>, n: &DNode) {
    let bi = m.tape_next();
    // SAFETY: the boundary's guards are reader fields of the live owner (`mount_bound`).
    unsafe { m.rt.el_bound_refresh(bi) };
    let clips = m.rt.el_bound_clips(bi);
    let prev = m.clip;
    for (i, a) in Items(n.first).enumerate() {
        m.clip = clips[i];
        update_dyn(m, std::iter::once(a));
    }
    m.clip = prev;
}

#[inline(always)]
fn ext(n: &DNode) -> Option<&Ext> {
    // SAFETY: a node's `ext` is null or its own, in this description's bump store.
    unsafe { n.ext.as_ref() }
}

#[inline(always)]
fn interactive(n: &DNode) -> Option<&Ext> {
    ext(n).filter(|x| !x.acts.is_null() || x.flags & (X_HOVER | X_FOCUS) != 0)
}

#[inline(always)]
fn acts(x: &Ext) -> impl Iterator<Item = &ActReq> {
    let mut a = x.acts;
    std::iter::from_fn(move || {
        // SAFETY: the route list of a node of this description.
        let r = unsafe { a.as_ref()? };
        a = r.next;
        Some(r)
    })
}

#[cold]
#[inline(never)]
fn wrong_owner<V>(r: &ActReq) -> ! {
    panic!("a route taking `&mut {}` is mounted in a view of `{}`: a route's receiver is the view that describes it", r.recv_name, std::any::type_name::<V>())
}

#[inline]
fn check<V: 'static>(r: &ActReq) {
    if r.recv != TypeId::of::<V>() {
        wrong_owner::<V>(r)
    }
}

fn key_code(k: Key) -> u32 {
    match k {
        Key::Char(c) => c as u32,
        Key::Tab => 0x11_0001,
        Key::Backspace => 0x11_0002,
        Key::Delete => 0x11_0003,
        Key::Enter => 0x11_0004,
        Key::Escape => 0x11_0005,
        Key::Left => 0x11_0006,
        Key::Right => 0x11_0007,
        Key::Up => 0x11_0008,
        Key::Down => 0x11_0009,
        Key::Home => 0x11_000a,
        Key::End => 0x11_000b,
        Key::PageUp => 0x11_000c,
        Key::PageDown => 0x11_000d,
    }
}

/// Install (or, on a re-description, rewrite) the routes of act `a` from the description.
fn set_routes<V: 'static>(m: &mut Mounter<'_, V>, a: u32, x: &Ext) {
    if !x.acts.is_null() {
        crate::input::root_owner_only(m, "a route");
    }
    let input = &mut m.rt.input;
    let act = &mut input.acts[a as usize];
    (act.click, act.press, act.hover, act.drag, act.key) = (None, None, None, None, None);
    let mut keys = 0;
    for r in acts(x) {
        check::<V>(r);
        // SAFETY: `check` proved the trampoline's receiver type is the dispatching owner's.
        let route = Some(unsafe { Route::raw(r.tramp, r.recipe) });
        // the list is most recent first: the last description of a slot wins
        let slot = match r.kind {
            A_CLICK => &mut act.click,
            A_PRESS => &mut act.press,
            A_HOVER => &mut act.hover,
            A_DRAG => &mut act.drag,
            A_ANY_KEY => &mut act.key,
            _ => {
                keys += 1;
                continue;
            }
        };
        if slot.is_none() {
            *slot = route;
        }
    }
    if keys != 0 || act.keys != 0 {
        let map = input.keymap(a);
        for r in acts(x).filter(|r| r.kind == A_KEY) {
            // SAFETY: as above.
            map.push((r.key, unsafe { Route::raw(r.tramp, r.recipe) }));
        }
    }
    let act = &mut input.acts[a as usize];
    if x.flags & X_HOVER != 0 {
        act.hover_bg = Some(x.hover);
    }
}

fn install<V: 'static>(m: &mut Mounter<'_, V>, before: usize, x: &Ext) {
    let focus = x.flags & X_FOCUS != 0 || acts(x).any(|r| matches!(r.kind, A_KEY | A_ANY_KEY));
    let a = crate::input::act_for(m, before, if focus { KF_FOCUS } else { 0 });
    if x.flags & X_HOVER != 0 {
        let inst = m.rt.input.acts[a as usize].inst;
        assert!(inst != NONE, "a hover style needs a painted box (bg, rounded or border)");
        let i = &m.rt.inst[inst as usize];
        let bordered = i.kind == KIND_BORDER;
        let base = if bordered { i.color2 } else { i.color };
        let act = &mut m.rt.input.acts[a as usize];
        (act.base, act.bordered) = (base, bordered);
    }
    set_routes(m, a, x);
}

#[inline(always)]
fn mix(h: u64, v: u64) -> u64 {
    (h ^ v).wrapping_mul(0x9e37_79b9_7f4a_7c15).rotate_left(27)
}

#[inline(always)]
fn mix_req(h: u64, w: *const WireReq) -> u64 {
    if w.is_null() {
        return mix(h, 0);
    }
    // SAFETY: as `wire`.
    let w = unsafe { &*w };
    mix(mix(h, w.head as usize as u64), w.patch as usize as u64)
}

/// What a re-description cannot change in place: the node's kind, paint, text metrics, reactive operations,
/// wires and interaction. Equal shapes update in place; a different shape remounts the item.
fn shape(n: &DNode) -> u64 {
    if cfg!(feature = "mutant_el_shape") {
        return 0;
    }
    let mut h = mix(0x51, n.kind as u64 | ((n.flags & (F_DRAW | F_BORDER | F_WRAP)) as u64) << 8 | (n.ops as u64) << 16);
    match n.kind {
        K_BOX => h = mix_req(h, n.ink),
        K_TEXT => h = mix_req(mix_req(mix(h, n.m[0].to_bits() as u64 | (n.m[1].to_bits() as u64) << 32), n.ink), n.tw),
        K_INPUT => h = mix(mix(h, n.text as usize as u64), n.m[0].to_bits() as u64 | (n.m[1].to_bits() as u64) << 32),
        K_PROD => {
            let p = prod(n);
            h = mix(mix(mix(h, p.src as usize as u64), p.dst as usize as u64), p.f as u64);
        }
        K_BOUND => {
            // the guard set and the policy: what the boundary's wires and clips were made for
            let b = bound(n);
            h = mix(h, b.policy as u64);
            for g in guards(b) {
                h = mix(h, g.reader as u64);
            }
        }
        K_IMAGE => {
            // SAFETY: as `image`.
            let b = unsafe { &*(n.text as *const crate::ui::ImageBox) };
            h = mix(mix(h, b.png.as_ptr() as usize as u64), b.png.len() as u64);
            // an image is mounted once and stepped over: its layout is part of what it is
            for c in n.spec.bytes().chunks_exact(8) {
                h = mix(h, u64::from_ne_bytes(c.try_into().unwrap()));
            }
        }
        _ => {}
    }
    if let Some(x) = ext(n) {
        for w in [x.wa, x.wtx, x.wty, x.ww, x.wh] {
            h = mix_req(h, w);
        }
        let mut kinds = (x.flags & (X_HOVER | X_FOCUS)) as u64;
        for r in acts(x) {
            kinds |= 0x100 << r.kind;
        }
        h = mix(h, kinds);
    }
    h
}

/// The node as a seal (`seal.rs`): every operand the walk would apply, in a fixed order, with the presence of each
/// optional part in the head word so no two descriptions write the same words. An El node is sealed directly from
/// its fields (no kernel builder is formed), which is what an unchanged row of a long list costs per turn.
#[inline(always)]
fn seal_node<S: Sealer>(n: &DNode, s: &mut S) {
    seal_at::<S, false>(n, s)
}

/// A node with children, out of line (the recursion; a row's leaves are sealed inline in `seal_at`).
#[inline(never)]
fn seal_deep<S: Sealer>(n: &DNode, s: &mut S) {
    seal_at::<S, false>(n, s)
}

/// `seal_node` for a child: a leaf inline, anything with children through `seal_deep`.
#[inline(always)]
fn seal_child<S: Sealer>(n: &DNode, s: &mut S) {
    if n.first.is_null() { seal_at::<S, true>(n, s) } else { seal_deep(n, s) }
}

/// `LEAF`: `n` has no children (so the checker stays in registers through a whole row).
///
/// Every node's seal starts with its block (`BLOCK` bytes, `arena.rs`): the head (kind, flags, operations, which
/// of the wires and the `Ext` are present), the two colours, the spec and the metrics. What follows is fixed by
/// the head, and the head (below `1 << 27`) is never a list's end word (`0x4b49_0000 | k`, `0x4752_0000 | k`),
/// so a node's seal is never a prefix of another's.
#[inline(always)]
fn seal_at<S: Sealer, const LEAF: bool>(n: &DNode, s: &mut S) {
    let x = ext(n);
    let has = |w: *const WireReq, bit: u8| if w.is_null() { 0 } else { bit };
    debug_assert_eq!(n.has, has(n.ink, H_INK) | has(n.tw, H_TW) | if x.is_some() { H_EXT } else { 0 }, "`DNode::has`");
    let b = n.block();
    // SAFETY: `BLOCK` plain bytes (`DNode::block`).
    s.word(unsafe { rd32(b, 0) });
    s.paint(n.paint);
    s.paint(n.bc);
    for o in (12..BLOCK).step_by(4) {
        // SAFETY: `BLOCK` plain bytes (`DNode::block`).
        s.word(unsafe { rd32(b, o) });
    }
    match n.kind {
        K_BOX => {
            if !n.ink.is_null() {
                seal_port(s, n.ink, true);
            }
            if n.ops != 0 {
                EOps { x: n.ext, ops: n.ops }.seal_ops(s);
            }
            let mut k = 0u32;
            if !LEAF {
                for c in Items(n.first) {
                    seal_child(c.n(), s);
                    k += 1;
                }
            }
            s.word(0x4b49_0000 | k);
        }
        K_TEXT => {
            if !n.ink.is_null() {
                seal_port(s, n.ink, true);
            }
            EText { ptr: n.text, len: n.len, flags: n.flags, w: n.tw }.seal_text(s);
        }
        K_GROUP | K_FRAG => {
            let mut k = 0u32;
            if !LEAF {
                for c in Items(n.first) {
                    seal_child(c.n(), s);
                    k += 1;
                }
            }
            s.word(0x4752_0000 | k);
        }
        K_PROD => seal_prod(n, s),
        K_BOUND => {
            // the guards and the policy (what its wires and verdict were made for), then its three arms. Which
            // arm shows is the guards' state, kept by the boundary's wires, not the description's: a boundary
            // described as before is stepped over, its verdict still current
            for w in bound_words(n) {
                s.word(w);
            }
            let mut k = 0u32;
            for c in Items(n.first) {
                seal_child(c.n(), s);
                k += 1;
            }
            s.word(0x4244_0000 | k);
        }
        _ => s.refuse(),
    }
    if let Some(x) = x {
        s.word(x.flags as u32);
        if x.flags & X_HOVER != 0 {
            s.paint(x.hover);
        }
        for r in acts(x) {
            s.word(r.kind as u32 | key_code(r.key) << 8);
            let t = r.tramp as usize as u64;
            s.word(t as u32);
            s.word((t >> 32) as u32);
            for c in r.recipe.bytes().chunks_exact(4) {
                s.word(u32::from_ne_bytes([c[0], c[1], c[2], c[3]]));
            }
        }
        s.word(0x4143_0000);
    }
}

impl<V: 'static> Element<V> for Item {
    fn mount(self, m: &mut Mounter<'_, V>) {
        let n = self.n();
        match n.kind {
            K_BOX | K_TEXT => {
                let before = m.recs.len();
                if n.kind == K_BOX {
                    lbox(n).mount(m)
                } else {
                    para(n).mount(m)
                }
                if let Some(x) = interactive(n) {
                    install(m, before, x);
                }
            }
            K_GROUP => mount_dyn_as(m, Items(n.first), true),
            K_FRAG => mount_dyn_as(m, Items(n.first), false),
            K_PROD => mount_prod(m, n),
            K_BOUND => mount_bound(m, n),
            K_INPUT => {
                let before = m.recs.len();
                let sig = input_signal::<V>(m, n);
                input_box(n, crate::ui::TextInput { sig, w: n.m[0], h: n.m[1] }).mount(m);
                if let Some(x) = interactive(n) {
                    install(m, before, x);
                }
            }
            _ => {
                let before = m.recs.len();
                image(n).mount(m);
                if let Some(x) = interactive(n) {
                    // the pixels are a decoration that takes no pointer: an image that hears one lets it hit, and
                    // its box (the act's record) is the next interactive record up the chain
                    for r in &mut m.recs[before + 1..] {
                        r.kind &= !bough_core::bough::hit::KF_NOHIT;
                    }
                    install(m, before, x);
                }
            }
        }
    }
    fn update(self, m: &mut Mounter<'_, V>) {
        let n = self.n();
        match n.kind {
            K_GROUP | K_FRAG => update_dyn(m, Items(n.first)),
            _ => {
                if n.has & (H_INK | H_TW) != 0 || n.ops != 0 {
                    resolve_ports(m, n);
                }
                // the element's first record carries its act (`input::act_for`)
                let act = interactive(n).map(|x| (x, m.rt.bough.handle_at(m.pos)));
                match n.kind {
                    K_BOX => lbox(n).update(m),
                    K_TEXT => para(n).update(m),
                    K_INPUT => <LBox<EOp<Color>, Opaque, EOps> as Element<V>>::update(input_box(n, Opaque), m),
                    // a producer keeps its wire and its last input; its identity is its shape
                    K_PROD => {}
                    K_BOUND => update_bound(m, n),
                    _ => update_opaque(m),
                }
                if let Some((x, Some(h))) = act {
                    let a = m.rt.input.act_of_handle(h);
                    debug_assert!(a != 0, "an interactive element kept its act");
                    if a != 0 {
                        set_routes(m, a, x);
                    }
                }
            }
        }
    }
    #[inline]
    fn dyn_key(&self) -> u64 {
        shape(self.n())
    }
    // a seal (`seal_at`) writes the kind, flags, operations, the identity of every wire and every route: all a
    // shape is made of
    const SEAL_HOLDS_KEY: bool = true;
    const SEAL_EQ: bool = true;
    #[inline]
    fn seal<S: Sealer>(&self, s: &mut S) {
        seal_node(self.n(), s)
    }
    #[inline(always)]
    fn seal_eq(&self, retained: &[u8]) -> Option<bool> {
        Some(seal_eq_node(self.n(), retained))
    }
    fn lower(self, l: &mut crate::lower::Lower<'_, '_, V>) {
        let n = self.n();
        match n.kind {
            K_BOX | K_TEXT => {
                let at = l.core.next();
                if n.kind == K_BOX {
                    lbox(n).lower(l)
                } else {
                    para(n).lower(l)
                }
                if let Some(x) = interactive(n).filter(|_| l.core.next() > at) {
                    lower_acts(l, at, x);
                }
            }
            K_GROUP | K_FRAG => Items(n.first).for_each(|c| c.lower(l)),
            K_INPUT => l.refuse("text_input"),
            K_PROD => l.refuse("produce"),
            K_BOUND => l.refuse("boundary"),
            _ => {
                let at = l.core.next();
                image(n).lower(l);
                if let Some(x) = interactive(n).filter(|_| l.core.next() > at) {
                    lower_acts(l, at, x);
                }
            }
        }
    }
}

fn lower_acts<V: 'static>(l: &mut crate::lower::Lower<'_, '_, V>, at: usize, x: &Ext) {
    use crate::lower::Kind;
    // as the kernel's act (`set_routes`): the most recent route of each slot, one route per key, and the any-key
    // route only for keys no route by key takes
    let mut slot: [Option<ActReq>; 6] = [None; 6];
    let mut keyed: Vec<ActReq> = Vec::new();
    for r in acts(x) {
        check::<V>(r);
        if r.kind == A_KEY {
            if !keyed.iter().any(|k| k.key == r.key) {
                keyed.push(*r);
            }
        } else if slot[r.kind as usize].is_none() {
            slot[r.kind as usize] = Some(*r);
        }
    }
    let call = |r: ActReq| {
        move |v: &mut V, e: &Ev| {
            // SAFETY: `check` proved the trampoline's receiver type is `V`.
            unsafe { (r.tramp)(v as *mut V as *mut u8, &r.recipe, e) }
        }
    };
    for (k, kind) in [(A_CLICK, Kind::Click), (A_PRESS, Kind::Press), (A_HOVER, Kind::Hover), (A_DRAG, Kind::Drag)] {
        if let Some(r) = slot[k as usize] {
            l.handler(at, kind, call(r));
        }
    }
    for r in &keyed {
        l.handler(at, Kind::Key, call(*r));
        l.last_handler_keys(Box::new([r.key]), true);
    }
    if let Some(r) = slot[A_ANY_KEY as usize] {
        l.handler(at, Kind::Key, call(r));
        l.last_handler_keys(keyed.iter().map(|k| k.key).collect(), false);
    }
    let focus = x.flags & X_FOCUS != 0 || !keyed.is_empty() || slot[A_ANY_KEY as usize].is_some();
    let node = l.core.node(at);
    if x.flags & X_HOVER != 0 {
        node.hover_bg = Some(x.hover);
        node.solid = true;
    }
    if focus {
        node.focusable = true;
        node.solid = true;
    }
}

// =============================================================================== the root

/// A described view: its root is one item of a dynamic part of its own, so a change of the root's kind
/// remounts the root, and a whole description can stand in any container of the kernel's.
impl<V: 'static> Element<V> for super::El<'_> {
    fn mount(self, m: &mut Mounter<'_, V>) {
        let root = Item(self.raw());
        let _w = Walk::hold();
        let _o = OwnerScope::of(m);
        mount_dyn_as(m, std::iter::once(root), false);
    }
    fn update(self, m: &mut Mounter<'_, V>) {
        let root = Item(self.raw());
        let _w = Walk::hold();
        // the field rule the seals of this walk ask (`installed`): this owner's
        let _o = OwnerScope::of(m);
        update_dyn(m, std::iter::once(root));
    }
    #[inline]
    fn dyn_key(&self) -> u64 {
        self.n();
        shape(Item(self.node).n())
    }
    #[inline]
    fn seal<S: Sealer>(&self, s: &mut S) {
        self.n();
        seal_node(Item(self.node).n(), s)
    }
    #[inline]
    fn seal_eq(&self, retained: &[u8]) -> Option<bool> {
        self.n();
        Some(seal_eq_node(Item(self.node).n(), retained))
    }
    const SEAL_HOLDS_KEY: bool = true;
    const SEAL_EQ: bool = true;
    fn lower(self, l: &mut crate::lower::Lower<'_, '_, V>)
    where
        V: 'static,
    {
        let root = Item(self.raw());
        let _w = Walk::hold();
        root.lower(l);
    }
}

#[allow(dead_code)]
const _: () = assert!(KF_INTERACT != KF_FOCUS);

// =============================================================================== the fast check

/// A retained seal being read by the fast check: `seal_at`'s words, compared in place with early exit. The
/// position travels by value (in and out of every function, null once something differed) and the bounds are
/// checked once per fixed-size run, so a row's check keeps both in registers.
type At = *const u8;

/// The 4 bytes at `p + o` as a word (`p + o + 4` is in bounds: checked by the caller).
#[inline(always)]
unsafe fn rd32(p: At, o: usize) -> u32 {
    // SAFETY: in bounds (caller); an unaligned read of plain bytes.
    unsafe { ptr::read_unaligned(p.add(o) as *const u32) }
}
#[inline(always)]
unsafe fn rd64(p: At, o: usize) -> u64 {
    // SAFETY: as `rd32`.
    unsafe { ptr::read_unaligned(p.add(o) as *const u64) }
}
#[inline(always)]
fn left(p: At, end: At) -> usize {
    end as usize - p as usize
}

/// `seal_port(w)` at `p` (a scalar port, or the one zero word of no port): then the position after it, else null.
/// The identity is compared inline; only a constant port's value (an eval: a call) is out of line.
#[inline(always)]
fn check_port(w: *const WireReq, p: At, end: At) -> At {
    if w.is_null() {
        // SAFETY: four bytes are left (checked).
        return if left(p, end) >= 4 && unsafe { rd32(p, 0) } == 0 { unsafe { p.add(4) } } else { ptr::null() };
    }
    // SAFETY: as `wire`.
    let w = unsafe { &*w };
    let (q, constant) = check_ident(w, p, end);
    if q.is_null() || !constant && !w.constant() { q } else { check_const_value(w, q, end) }
}
/// A constant scalar port's value now against the sealed one.
#[cold]
#[inline(never)]
fn check_const_value(w: *const WireReq, q: At, end: At) -> At {
    // SAFETY: as `wire`; four bytes are left (checked).
    if left(q, end) >= 4 && unsafe { rd32(q, 0) == (*w).scalar() } { unsafe { q.add(4) } } else { ptr::null() }
}

/// `seal_text_port(w)` at `p`.
#[inline(always)]
fn check_text_port(w: *const WireReq, p: At, end: At) -> At {
    // SAFETY: as `wire` (a text port's request is never null here).
    let w = unsafe { &*w };
    let (q, constant) = check_ident(w, p, end);
    if q.is_null() || !constant && !w.constant() { q } else { check_const_text(w, q, end) }
}
/// A constant text port's text now against the sealed one.
#[cold]
#[inline(never)]
fn check_const_text(w: *const WireReq, q: At, end: At) -> At {
    // SAFETY: as `wire`.
    unsafe { &*w }.text(|t| {
        let n = t.len();
        // SAFETY: `4 + n` bytes are left (checked).
        if left(q, end) >= 4 + n && unsafe { rd32(q, 0) } == n as u32 && unsafe { eq_n(q.add(4), t.as_ptr(), n) } { unsafe { q.add(4 + n) } } else { ptr::null() }
    })
}

/// A port's identity words at `p` (`seal_ident`): the position after them (null: they differ), and whether the
/// retained seal says the port is a constant (`CONST`: its value follows). A retained `WIRED` is trusted as a
/// field verdict (see "Lazy port values"); whether that port now needs a value is the update's `P_*` state.
#[inline(always)]
fn check_ident(w: &WireReq, p: At, end: At) -> (At, bool) {
    if left(p, end) < 36 {
        return (ptr::null(), false);
    }
    // SAFETY: 36 bytes are left (checked); a recipe is all initialised (`Recipe::with_at`).
    unsafe {
        let (a, f) = (w.head as usize as u64, w.patch as usize as u64);
        let magic = rd32(p, 0);
        let words = (rd32(p, 4) ^ a as u32) | (rd32(p, 8) ^ (a >> 32) as u32) | (rd32(p, 12) ^ f as u32) | (rd32(p, 16) ^ (f >> 32) as u32);
        let r = w.recipe.as_ptr();
        let recipe = (rd64(p, 20) ^ rd64(r, 0)) | (rd64(p, 28) ^ rd64(r, 8));
        if words != 0 || recipe != 0 || (magic != WIRED && magic != CONST) {
            return (ptr::null(), false);
        }
        (p.add(36), magic == CONST)
    }
}

/// `n`'s block against the `BLOCK` bytes at `p` (in bounds: checked by the caller).
#[inline(always)]
unsafe fn eq_block(n: &DNode, p: At) -> bool {
    let b = n.block();
    let mut d = 0u64;
    for i in 0..BLOCK / 8 {
        // SAFETY: `BLOCK` bytes at both (`DNode::block`, caller).
        d |= unsafe { rd64(p, i * 8) ^ rd64(b, i * 8) };
    }
    const { assert!(BLOCK % 8 == 0) };
    d == 0
}

/// `a[..n] == b[..n]` (`n` bytes readable at both), inline at every length.
#[inline(always)]
unsafe fn eq_n(a: At, b: At, n: usize) -> bool {
    // SAFETY: every read is inside `[0, n)` of both (overlapping words for the tail).
    unsafe {
        if n > 16 {
            let mut d = 0u64;
            let mut o = 0;
            while o + 8 <= n {
                d |= rd64(a, o) ^ rd64(b, o);
                o += 8;
            }
            d |= rd64(a, n - 8) ^ rd64(b, n - 8);
            d == 0
        } else if n >= 8 {
            (rd64(a, 0) ^ rd64(b, 0)) | (rd64(a, n - 8) ^ rd64(b, n - 8)) == 0
        } else if n >= 4 {
            (rd32(a, 0) ^ rd32(b, 0)) | (rd32(a, n - 4) ^ rd32(b, n - 4)) == 0
        } else if n >= 2 {
            let h = |p: At, o: usize| ptr::read_unaligned(p.add(o) as *const u16);
            (h(a, 0) ^ h(b, 0)) | (h(a, n - 2) ^ h(b, n - 2)) == 0
        } else {
            n == 0 || *a == *b
        }
    }
}

/// Whether `seal_at` would write exactly the bytes at `p` (then: the position after them; else null).
#[inline(always)]
fn check_at<const LEAF: bool>(n: &DNode, p: At, end: At) -> At {
    // SAFETY: `BLOCK` bytes are left (checked).
    if left(p, end) < BLOCK || !unsafe { eq_block(n, p) } {
        return ptr::null();
    }
    let p = unsafe { p.add(BLOCK) };
    // a node with a fill, ink or text port is checked out of line: a list's plain rows have none, and their
    // check carries no port code (whose evals are calls)
    if n.has & (H_INK | H_TW) != 0 {
        return check_ported::<LEAF>(n, p, end);
    }
    check_body::<LEAF, false>(n, p, end)
}

#[inline(never)]
fn check_ported<const LEAF: bool>(n: &DNode, p: At, end: At) -> At {
    check_body::<LEAF, true>(n, p, end)
}

/// `check_at` after the block. `PORTS`: the node may have a fill, ink or text port (else its `ink` and `tw` are
/// null, `DNode::has`).
#[inline(always)]
fn check_body<const LEAF: bool, const PORTS: bool>(n: &DNode, p: At, end: At) -> At {
    let mut p = p;
    match n.kind {
        K_BOX => {
            if PORTS && !n.ink.is_null() {
                p = check_port(n.ink, p, end);
                if p.is_null() {
                    return p;
                }
            }
            if n.ops != 0 {
                p = check_ops(n, p, end);
                if p.is_null() {
                    return p;
                }
            }
            p = check_kids::<LEAF>(n, p, end, 0x4b49_0000);
        }
        K_TEXT => {
            if PORTS && !n.ink.is_null() {
                p = check_port(n.ink, p, end);
                if p.is_null() {
                    return p;
                }
            }
            if n.flags & F_INT != 0 {
                // SAFETY: eight bytes are left (checked).
                if left(p, end) < 8 || unsafe { (rd32(p, 0) ^ n.len as u32) | (rd32(p, 4) ^ (n.len >> 32) as u32) } != 0 {
                    return ptr::null();
                }
                p = unsafe { p.add(8) };
            } else {
                let len = n.len as usize;
                // SAFETY: `4 + len` bytes are left (checked); the node's text is `len` bytes (as `EText::with`).
                if left(p, end) < 4 + len || unsafe { rd32(p, 0) } != len as u32 || !unsafe { eq_n(p.add(4), n.text, len) } {
                    return ptr::null();
                }
                p = unsafe { p.add(4 + len) };
            }
            if PORTS && !n.tw.is_null() {
                // a text port (its node's text is the empty placeholder): its identity, and a constant's text
                p = check_text_port(n.tw, p, end);
            }
        }
        K_GROUP | K_FRAG => p = check_kids::<LEAF>(n, p, end, 0x4752_0000),
        K_PROD => p = check_prod(n, p, end),
        K_BOUND => p = check_bound(n, p, end),
        _ => return ptr::null(),
    }
    if p.is_null() || n.ext.is_null() {
        return p;
    }
    // SAFETY: a node's `ext` is its own (as `ext`).
    check_ext(unsafe { &*n.ext }, p, end)
}

#[inline(always)]
fn check_kids<const LEAF: bool>(n: &DNode, p: At, end: At, tag: u32) -> At {
    let mut p = p;
    let mut k = 0u32;
    if !LEAF {
        for it in Items(n.first) {
            let ch = it.n();
            p = if ch.first.is_null() { check_at::<true>(ch, p, end) } else { check_deep(ch, p, end) };
            if p.is_null() {
                return p;
            }
            k += 1;
        }
    }
    // SAFETY: four bytes are left (checked).
    if left(p, end) >= 4 && unsafe { rd32(p, 0) } == tag | k { unsafe { p.add(4) } } else { ptr::null() }
}

/// `seal_at`'s words of a boundary after its block: its identity, then its arms.
#[cold]
#[inline(never)]
fn check_bound(n: &DNode, p: At, end: At) -> At {
    let mut p = p;
    for w in bound_words(n) {
        // SAFETY: four bytes are left (checked).
        if left(p, end) < 4 || unsafe { rd32(p, 0) } != w {
            return ptr::null();
        }
        p = unsafe { p.add(4) };
    }
    let mut k = 0u32;
    for it in Items(n.first) {
        let ch = it.n();
        p = if ch.first.is_null() { check_at::<true>(ch, p, end) } else { check_deep(ch, p, end) };
        if p.is_null() {
            return p;
        }
        k += 1;
    }
    // SAFETY: four bytes are left (checked).
    if left(p, end) >= 4 && unsafe { rd32(p, 0) } == 0x4244_0000 | k { unsafe { p.add(4) } } else { ptr::null() }
}

#[inline(never)]
fn check_deep(n: &DNode, p: At, end: At) -> At {
    check_at::<false>(n, p, end)
}

#[cold]
fn check_ops(n: &DNode, p: At, end: At) -> At {
    // SAFETY: `ops != 0` only with an `Ext`.
    let x = unsafe { &*n.ext };
    let mut p = p;
    for (bit, v, w) in [(OP_ALPHA, x.alpha, x.wa), (OP_TX, x.tx, x.wtx), (OP_TY, x.ty, x.wty), (OP_W, x.w, x.ww), (OP_H, x.h, x.wh)] {
        if n.ops & bit != 0 {
            // SAFETY: four bytes are left (checked).
            if left(p, end) < 4 || unsafe { rd32(p, 0) } != v.to_bits() {
                return ptr::null();
            }
            p = check_port(w, unsafe { p.add(4) }, end);
            if p.is_null() {
                return p;
            }
        }
    }
    p
}

#[inline(never)]
fn check_ext(x: &Ext, p: At, end: At) -> At {
    let mut p = p;
    let word = |p: At, w: u32| -> At {
        // SAFETY: four bytes are left (checked).
        if left(p, end) >= 4 && unsafe { rd32(p, 0) } == w { unsafe { p.add(4) } } else { ptr::null() }
    };
    p = word(p, x.flags as u32);
    if p.is_null() {
        return p;
    }
    if x.flags & X_HOVER != 0 {
        p = word(p, x.hover);
        if p.is_null() {
            return p;
        }
    }
    for r in acts(x) {
        if left(p, end) < 28 {
            return ptr::null();
        }
        let t = r.tramp as usize as u64;
        // SAFETY: 28 bytes are left (checked); a recipe is all initialised (`Recipe::with_at`).
        let ok = unsafe { (rd32(p, 0) ^ (r.kind as u32 | key_code(r.key) << 8)) | (rd32(p, 4) ^ t as u32) | (rd32(p, 8) ^ (t >> 32) as u32) == 0 && eq_n(p.add(12), r.recipe.as_ptr(), 16) };
        if !ok {
            return ptr::null();
        }
        p = unsafe { p.add(28) };
    }
    word(p, 0x4143_0000)
}

/// `Element::seal_eq` for a node.
#[inline(always)]
fn seal_eq_node(n: &DNode, retained: &[u8]) -> bool {
    let p = retained.as_ptr();
    // SAFETY: one past the end of `retained`.
    let end = unsafe { p.add(retained.len()) };
    let q = if n.first.is_null() { check_at::<true>(n, p, end) } else { check_at::<false>(n, p, end) };
    q == end && !q.is_null()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::el::{El, Source, column, div, row, text};
    use crate::seal::{SealCheck, SealWrite};

    /// The fast check accepts exactly what the streaming checker calls equal, on descriptions that differ by
    /// one operand each, including a field port and a non-field port.
    #[test]
    fn the_fast_check_agrees_with_the_seal() {
        let sig = crate::signal::Signal::new(crate::color::rgb(0x123456));
        let f = crate::signal::Signal::new(0.5f32);
        struct Owner {
            field: crate::signal::Signal<u32>,
            far: Vec<crate::signal::Signal<u32>>,
        }
        let mut o = Owner { field: crate::signal::Signal::new(0), far: vec![crate::signal::Signal::new(0)] };
        fn describe<'a>(o: &'a Owner, v: u32, sig: &'a crate::signal::Signal<Color>, f: &'a crate::signal::Signal<f32>) -> El<'a> {
            let t = |s: &'static str| text(s).fg(crate::color::rgb(0x010203 * (v & 1)));
            let field = column((text(o.field.map(|n| format!("field {n}"))).w(80), div().size(8, 8).bg(o.field.map(|n| crate::color::rgb(*n))).w(80)));
            column((
                row((t("a"), text(v % 3), text(format!("n{}", v % 2)))).gap((v % 4) as f32),
                if v % 5 == 0 { div().size(10, 10).bg(sig) } else { div().size(10 + v % 2, 10).bg(crate::color::rgb(v % 7)).rounded(v % 3) },
                div().size(4, 4).bg(sig).opacity(f.map(|x| x * 2.0)),
                (v % 2 == 0).then(|| text("maybe")),
                (0..v % 4).map(|i| div().size(i + 1, 2).bg(crate::color::rgb(i))),
                text("t").font(12 + v % 2, 18),
                // texts of 8..=16 and of more than 16 bytes, differing in their last byte
                text(format!("Row label {}", v % 2)),
                text(format!("a label of more than sixteen bytes {}", v % 3)),
                // a field signal's value changes without changing its description; a value behind a Vec changes
                // its description. Both are a map's source (and both seal their recipe). The field ports are
                // nested so only a leaf fast-check can miss one of their values.
                field,
                text(o.far[0].map(|n| format!("far {n}"))).w(80),
                div().size(8, 8).bg(sig).opacity(f.map(move |x| x * (v % 3) as f32)),
            ))
            .p(v % 2)
        }
        fn with_state(item: Item, state: u8) {
            let n = item.n();
            if !n.ink.is_null() {
                // SAFETY: the test owns this description and has no other reference to its request.
                unsafe { (*(n.ink as *mut WireReq)).state = state };
            }
            with_state_inner(n, state);
        }
        fn with_state_inner(n: &DNode, state: u8) {
            for c in Items(n.first) {
                let c = c.n();
                if !c.ink.is_null() {
                    // SAFETY: as `with_state`.
                    unsafe { (*(c.ink as *mut WireReq)).state = state };
                }
                if !c.tw.is_null() {
                    // SAFETY: as `with_state`.
                    unsafe { (*(c.tw as *mut WireReq)).state = state };
                }
                with_state_inner(c, state);
            }
        }
        let mut descs = crate::layout::SpecTable::default();
        for a in 0..24u32 {
            for far in 0..3u32 {
                o.field.set(a % 7);
                o.far[0].set(far);
                let mut bytes = Vec::new();
                {
                    let _scope = OwnerScope::raw(&o);
                    let ea = describe(&o, a, &sig, &f);
                    seal_node(Item(ea.raw()).n(), &mut SealWrite { out: &mut bytes, descs: &mut descs, ok: true });
                }
                // The same field port described with a new field value: the streaming seal and the fast check
                // agree in the normal build (the wire owns it) and in the eager mutant (the value participates).
                assert_ne!((a + 1) % 7, a % 7, "the field's new value is different");
                o.field.set((a + 1) % 7);
                {
                    let _scope = OwnerScope::raw(&o);
                    let eb = describe(&o, a, &sig, &f);
                    let root = eb.raw();
                    let nb = Item(root);
                    let mut sc = SealCheck::new(&bytes, &descs);
                    seal_node(nb.n(), &mut sc);
                    assert_eq!(seal_eq_node(nb.n(), &bytes), sc.equal(), "new field value");
                    if !cfg!(feature = "mutant_el_eager_port") {
                        assert!(sc.equal(), "new field value: the wire owns it");
                    }
                    // An update resolves each port before its item's fast check. For a field port that is
                    // `P_KEEP`; for a non-field (and every port in the eager mutant) it is `P_CONST`.
                    let state = if cfg!(feature = "mutant_el_eager_port") { P_CONST } else { P_KEEP };
                    with_state(Item(root), state);
                    let mut resolved = SealCheck::new(&bytes, &descs);
                    seal_node(nb.n(), &mut resolved);
                    assert_eq!(seal_eq_node(nb.n(), &bytes), resolved.equal(), "new field value, after port resolution");
                    if cfg!(feature = "mutant_el_eager_port") {
                        assert!(!resolved.equal(), "the eager mutant makes the field's value part of its seal");
                    }
                }
                for b in 0..24u32 {
                    for new_far in 0..3u32 {
                        o.field.set(b % 7);
                        o.far[0].set(new_far);
                        let _scope = OwnerScope::raw(&o);
                        let eb = describe(&o, b, &sig, &f);
                        let nb = Item(eb.raw());
                        let mut sc = SealCheck::new(&bytes, &descs);
                        seal_node(nb.n(), &mut sc);
                        assert_eq!(seal_eq_node(nb.n(), &bytes), sc.equal(), "descriptions {a}/{b}, far {far}/{new_far}");
                        assert_eq!(sc.equal(), a == b && far == new_far, "descriptions {a}/{b}, far {far}/{new_far}");
                    }
                }
                o.far[0].set(far);
            }
        }
    }

    /// The eager mutant's oracle: after an update has resolved the same request as a constant, the fast check
    /// sees the field's new value and the streaming seal agrees. The mutant's regression is proved by el_lazy.
    #[cfg(feature = "mutant_el_eager_port")]
    #[test]
    fn the_eager_mutant_makes_a_fields_new_value_part_of_its_seal() {
        struct Owner {
            field: crate::signal::Signal<u32>,
        }
        let mut o = Owner { field: crate::signal::Signal::new(1) };
        let mut descs = crate::layout::SpecTable::default();
        let mut bytes = Vec::new();
        let item = {
            let _scope = OwnerScope::raw(&o);
            let e = div().size(8, 8).bg(o.field.map(|n| crate::color::rgb(*n)));
            let item = Item(e.raw());
            seal_node(item.n(), &mut SealWrite { out: &mut bytes, descs: &mut descs, ok: true });
            item
        };
        o.field.set(2);
        // SAFETY: this test owns the description's request and has no other reference to it.
        unsafe { (*(item.n().ink as *mut WireReq)).state = P_CONST };
        let mut sc = SealCheck::new(&bytes, &descs);
        seal_node(item.n(), &mut sc);
        assert!(!sc.equal(), "the eager mutant seals the field's value");
        assert_eq!(seal_eq_node(item.n(), &bytes), sc.equal(), "the fast check agrees with the streaming seal");
    }

    /// The same value written to any two different fields of a node's block (spec, metrics, flags) never
    /// seals alike.
    #[test]
    fn block_fields_seal_apart() {
        use crate::layout::{Align, Justify, Styled};
        let make = |v: u32| -> super::super::El<'static> {
            let b = div();
            match v {
                0 => b,
                1 => b.w(3),
                2 => b.h(3),
                3 => b.min_w(3),
                4 => b.min_h(3),
                5 => b.px(3),
                6 => b.py(3),
                7 => b.margin(3, 0, 0, 0),
                8 => b.margin(0, 0, 0, 3),
                9 => b.gap(3),
                10 => b.grow(3.0),
                11 => b.shrink(3.0),
                12 => b.abs(3, 0),
                13 => b.abs(0, 3),
                14 => b.rounded(3),
                15 => b.border(3, crate::color::rgb(0)),
                16 => b.items(Align::Center),
                17 => b.justify(Justify::End),
                18 => b.w_pct(3.0),
                19 => b.shrink(0.0),
                20 => b.relative(),
                21 => b.clip_overflow(),
                22 => b.layout(crate::ui::Flex::row().gap(3.0)),
                _ => {
                    let mut b = b;
                    b.spec_mut().grow = 4.0;
                    b
                }
            }
        };
        let mut descs = crate::layout::SpecTable::default();
        for a in 0..24 {
            let mut bytes = Vec::new();
            let ea = make(a);
            seal_node(Item(ea.raw()).n(), &mut SealWrite { out: &mut bytes, descs: &mut descs, ok: true });
            for b in 0..24 {
                let eb = make(b);
                let nb = Item(eb.raw());
                let mut sc = SealCheck::new(&bytes, &descs);
                seal_node(nb.n(), &mut sc);
                assert_eq!(seal_eq_node(nb.n(), &bytes), sc.equal(), "descriptions {a} and {b}");
                assert_eq!(sc.equal(), a == b, "descriptions {a} and {b}");
            }
            drop(Walk::enter());
        }
    }
}
