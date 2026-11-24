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
use crate::elem::{decimal, Apply, Element, IntoProp, Mounter, TextSrc};
use crate::input::{Ev, Key, Route};
use crate::layout::{Dim, LBox, Para, Spec};
use crate::rt::*;
use crate::seal::Sealer;
use bough_core::bough::hit::{KF_FOCUS, KF_INTERACT};
use std::any::TypeId;
use std::ptr::{self, NonNull};

// =============================================================================== erased operands

/// A port's operand as the walk hands it to a kernel builder: the described value and the wire the setter
/// asked for (installed when its cell is a field of the mounting view).
pub(crate) struct EOp<P> {
    v: P,
    w: *const WireReq,
}

/// Installs `w` on `node` if its cell is a field of the mounting view.
#[inline]
fn wire<V>(m: &mut Mounter<'_, V>, w: *const WireReq, node: u32) {
    if !w.is_null() {
        // SAFETY: `w` was written this description by a setter: `head` is the head of a live cell of the type
        // `ty` names, and `patch` reads that cell type with this recipe (`plug.rs`).
        unsafe {
            let w = &*w;
            m.wire_field(&*w.head, w.ty, w.patch, node, w.recipe);
        }
    }
}

/// A wire's identity in a seal: the cell's address, the patch and its recipe.
#[inline(always)]
fn seal_req<S: Sealer>(s: &mut S, w: *const WireReq) {
    if w.is_null() {
        s.word(0);
        return;
    }
    // SAFETY: as `wire`.
    let w = unsafe { &*w };
    s.word(0x5749_5245);
    let (a, p) = (w.head as usize as u64, w.patch as usize as u64);
    s.word(a as u32);
    s.word((a >> 32) as u32);
    s.word(p as u32);
    s.word((p >> 32) as u32);
    for c in w.recipe.bytes().chunks_exact(4) {
        s.word(u32::from_ne_bytes([c[0], c[1], c[2], c[3]]));
    }
}

/// How a scalar operand's value enters a seal.
pub(crate) trait SealVal: Copy + 'static {
    fn seal_val<S: Sealer>(self, s: &mut S);
}
impl SealVal for Color {
    #[inline(always)]
    fn seal_val<S: Sealer>(self, s: &mut S) {
        s.paint(self.0)
    }
}
impl SealVal for f32 {
    #[inline(always)]
    fn seal_val<S: Sealer>(self, s: &mut S) {
        s.f32(self)
    }
}

impl<P: SealVal, V> IntoProp<P, V> for EOp<P> {
    #[inline]
    fn lower<K: Apply<P = P>>(self, m: &mut Mounter<'_, V>, node: u32) -> P {
        wire(m, self.w, node);
        self.v
    }
    #[inline(always)]
    fn value(self) -> P {
        self.v
    }
    #[inline(always)]
    fn seal<S: Sealer>(&self, s: &mut S) {
        // the value too: a wire whose cell is not a field of the view was not installed, and the value then
        // reaches the record only through a re-description
        self.v.seal_val(s);
        seal_req(s, self.w);
    }
}

/// A text operand: borrowed or copied bytes, or an integer, and its wire request.
pub(crate) struct EText {
    ptr: *const u8,
    len: u64,
    flags: u8,
    w: *const WireReq,
}

impl EText {
    #[inline(always)]
    fn with<R>(&self, f: impl FnOnce(&str) -> R) -> R {
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
        if self.flags & F_INT != 0 {
            s.int(self.len);
        } else {
            // SAFETY: as `with`.
            s.bytes(unsafe { std::slice::from_raw_parts(self.ptr, self.len as usize) });
        }
        if !self.w.is_null() {
            seal_req(s, self.w);
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
        if self.ops & OP_ALPHA != 0 {
            ApplyOpacity::apply(rt, node, &x.alpha);
        }
        if self.ops & OP_TX != 0 {
            ApplyTx::apply(rt, node, &x.tx);
        }
        if self.ops & OP_TY != 0 {
            ApplyTy::apply(rt, node, &x.ty);
        }
        if self.ops & OP_W != 0 {
            ApplyBoxW::apply(rt, lid, &x.w);
        }
        if self.ops & OP_H != 0 {
            ApplyBoxH::apply(rt, lid, &x.h);
        }
    }
    fn lower_ops(self, spec: &mut Spec, o: &mut BoxLow) {
        if self.ops == 0 {
            return;
        }
        let x = self.x();
        if self.ops & OP_ALPHA != 0 {
            o.alpha = x.alpha;
        }
        if self.ops & OP_TX != 0 {
            o.shift.0 = x.tx;
        }
        if self.ops & OP_TY != 0 {
            o.shift.1 = x.ty;
        }
        if self.ops & OP_W != 0 {
            spec.w = Dim::Pt(x.w);
        }
        if self.ops & OP_H != 0 {
            spec.h = Dim::Pt(x.h);
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
                seal_req(s, w);
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
    LBox {
        spec: n.spec,
        draw: n.flags & F_DRAW != 0,
        bg: EOp { v: Color(n.paint), w: n.ink },
        radius: n.m[0],
        border: if n.flags & F_BORDER != 0 { Some((n.m[1], Color(n.bc))) } else { None },
        ch: EKids(n.first),
        x: EOps { x: n.ext, ops: n.ops },
    }
}

#[inline(always)]
fn para(n: &DNode) -> Para<EText, EOp<Color>> {
    Para {
        s: EText { ptr: n.text, len: n.len, flags: n.flags, w: n.tw },
        spec: n.spec,
        size: n.m[0],
        lh: n.m[1],
        color: EOp { v: Color(n.paint), w: n.ink },
        wrap: n.flags & F_WRAP != 0,
    }
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
    if n.first.is_null() {
        seal_at::<S, true>(n, s)
    } else {
        seal_deep(n, s)
    }
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
                seal_req(s, n.ink);
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
                seal_req(s, n.ink);
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
                // the element's first record carries its act (`input::act_for`)
                let act = interactive(n).map(|x| (x, m.rt.bough.handle_at(m.pos)));
                match n.kind {
                    K_BOX => lbox(n).update(m),
                    K_TEXT => para(n).update(m),
                    K_INPUT => <LBox<EOp<Color>, Opaque, EOps> as Element<V>>::update(input_box(n, Opaque), m),
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
        mount_dyn_as(m, std::iter::once(root), false);
    }
    fn update(self, m: &mut Mounter<'_, V>) {
        let root = Item(self.raw());
        let _w = Walk::hold();
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

/// `seal_req(w)` at `p`: the wire's identity, or the one zero word of no wire.
#[inline(always)]
fn check_req(w: *const WireReq, p: At, end: At) -> At {
    if w.is_null() {
        // SAFETY: four bytes are left (checked).
        return if left(p, end) >= 4 && unsafe { rd32(p, 0) } == 0 { unsafe { p.add(4) } } else { ptr::null() };
    }
    if left(p, end) < 36 {
        return ptr::null();
    }
    // SAFETY: as `wire`; 36 bytes are left (checked); a recipe is all initialised (`Recipe::with_at`).
    unsafe {
        let w = &*w;
        let (a, f) = (w.head as usize as u64, w.patch as usize as u64);
        let words = (rd32(p, 0) ^ 0x5749_5245) | (rd32(p, 4) ^ a as u32) | (rd32(p, 8) ^ (a >> 32) as u32) | (rd32(p, 12) ^ f as u32) | (rd32(p, 16) ^ (f >> 32) as u32);
        let r = w.recipe.as_ptr();
        let recipe = (rd64(p, 20) ^ rd64(r, 0)) | (rd64(p, 28) ^ rd64(r, 8));
        if words == 0 && recipe == 0 { p.add(36) } else { ptr::null() }
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
    let mut p = unsafe { p.add(BLOCK) };
    match n.kind {
        K_BOX => {
            if !n.ink.is_null() {
                p = check_req(n.ink, p, end);
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
            if !n.ink.is_null() {
                p = check_req(n.ink, p, end);
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
            if !n.tw.is_null() {
                p = check_req(n.tw, p, end);
            }
        }
        K_GROUP | K_FRAG => p = check_kids::<LEAF>(n, p, end, 0x4752_0000),
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
            p = check_req(w, unsafe { p.add(4) }, end);
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
    use crate::el::{column, div, row, text, Source};
    use crate::seal::{SealCheck, SealWrite};

    /// The fast check accepts exactly what the streaming checker calls equal, on descriptions that differ by
    /// one operand each.
    #[test]
    fn the_fast_check_agrees_with_the_seal() {
        let sig = crate::signal::Signal::new(crate::color::rgb(0x123456));
        let f = crate::signal::Signal::new(0.5f32);
        let make = |v: u32| -> super::super::El<'_> {
            let t = |s: &'static str| text(s).fg(crate::color::rgb(0x010203 * (v & 1)));
            column((
                row((t("a"), text(v % 3), text(format!("n{}", v % 2)))).gap((v % 4) as f32),
                if v % 5 == 0 { div().size(10, 10).bg(&sig) } else { div().size(10 + v % 2, 10).bg(crate::color::rgb(v % 7)).rounded(v % 3) },
                div().size(4, 4).bg(&sig).opacity(f.map(|x| x * 2.0)),
                (v % 2 == 0).then(|| text("maybe")),
                (0..v % 4).map(|i| div().size(i + 1, 2).bg(crate::color::rgb(i))),
                text("t").font(12 + v % 2, 18),
                // texts of 8..=16 and of more than 16 bytes, differing in their last byte
                text(format!("Row label {}", v % 2)),
                text(format!("a label of more than sixteen bytes {}", v % 3)),
            ))
            .p(v % 2)
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
                if cfg!(miri) {
                    // Rust does not promise that one function cast to a pointer twice gives one address, and
                    // Miri gives a fresh one each time: a seal holds patch and trampoline addresses, so equal
                    // descriptions may seal apart (a missed skip, never a wrong one). Different ones never
                    // seal alike.
                    assert!(a == b || !sc.equal(), "descriptions {a} and {b} (no two are alike)");
                } else {
                    assert_eq!(sc.equal(), a == b, "descriptions {a} and {b} (no two are alike)");
                }
            }
            drop(Walk::enter());
        }
    }

    /// Map closures whose captures have padding (which `reflect` cannot prove absent for any closure) are
    /// sealed by their frozen bytes: two different values never seal alike, the fast check agrees with the
    /// streaming one, and padding-free captures always seal alike. (Equal padded values may differ in their
    /// padding bytes and then only miss a seal: not asserted.)
    #[test]
    #[cfg_attr(miri, ignore = "the recipe freeze is inline asm, which Miri cannot execute (its argument: an asm block's outputs are initialised, rt.rs)")]
    fn padded_captures_seal_by_value() {
        #[derive(Clone, Copy)]
        struct Pad {
            a: u8,
            b: u32,
        }
        assert!(!crate::reflect::padding_free::<Pad>(), "the test's capture has padding");
        let f = crate::signal::Signal::new(0.5f32);
        let padded = |v: u32| -> super::super::El<'_> {
            let p = Pad { a: (v % 3) as u8, b: v / 3 };
            let q = ((v % 2) as u8, 7u32);
            div().size(4, 4).opacity(f.map(move |x| x * p.b as f32 + p.a as f32 + q.0 as f32 + q.1 as f32))
        };
        let plain = |v: u32| -> super::super::El<'_> {
            let p = (v % 3, v / 3);
            div().size(4, 4).opacity(f.map(move |x| x * p.1 as f32 + p.0 as f32))
        };
        fn sweep<'a>(name: &str, make: impl Fn(u32) -> super::super::El<'a>) -> u32 {
            let mut descs = crate::layout::SpecTable::default();
            let mut hits = 0;
            for a in 0..9 {
                let mut bytes = Vec::new();
                let ea = make(a);
                let mut w = SealWrite { out: &mut bytes, descs: &mut descs, ok: true };
                seal_node(Item(ea.raw()).n(), &mut w);
                assert!(w.ok, "{name}: a map over a capturing closure seals (not refused)");
                for b in 0..9 {
                    let nb = Item(make(b).raw());
                    let mut sc = SealCheck::new(&bytes, &descs);
                    seal_node(nb.n(), &mut sc);
                    assert_eq!(seal_eq_node(nb.n(), &bytes), sc.equal(), "{name} {a} {b}: fast check = streaming check");
                    if a != b {
                        assert!(!sc.equal(), "{name} {a} {b}: different captures seal apart");
                    } else {
                        hits += sc.equal() as u32;
                    }
                }
                drop(Walk::enter());
            }
            hits
        }
        assert_eq!(sweep("plain", plain), 9, "equal padding-free captures seal alike");
        let hits = sweep("padded", padded);
        eprintln!("padded: {hits}/9 equal descriptions sealed alike");
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
