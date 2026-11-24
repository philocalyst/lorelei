//! Keyed collections as children (`designs/reactivity/final/02-elements.md` §4, §4.1, L8; `01-state.md` §4):
//! `column(&self.items)` (`T: View`: each row is the item's own `view`) and `column(self.items.each(f))` (the
//! explicit projection, `f: fn(&T) -> El` or `fn(&T, Slot<T>) -> El`).
//!
//! A list child is lowered onto the kernel's list region (`listview.rs`) with a row mounter of its own
//! ([`mount_elrow`]): one region per list child, one owner class for its rows, the list's delta log replayed on
//! the window, so a splice mounts only the new row, a move or a permutation moves the rows' records without
//! remounting, and the row identity (and what the kernel keeps per row: hover, focus, wires) is the slot's
//! across any permutation. An edit (`List::edit`) settles the row's wires and, when the region's owner
//! re-describes, re-describes that row alone against its own tape (`Rt::lr_redescribe`).
//!
//! A list child must be a field of the view that describes it: the borrowed list's address is proven a field
//! by the owner type's cell table (`reflect::cells`), the same proof a signal port uses. A list elsewhere (behind
//! a `Vec`, in another object) is a located panic naming the list and the owner: it would have no wire, and a
//! silent ordinal fallback would lose the identity the author asked for (the ordinal spelling is
//! `list.iter().map(..)`).
//!
//! The region is a full-width vertical stack in its parent: the parent box (a column) must be the list's
//! direct parent, and its gap is the rows' gap. A list in a row (left to right) is refused by name: the kernel's
//! region stacks its rows top to bottom.
use super::arena::*;
use super::{ChildSeq, El, View};
use crate::elem::{Element, Mounter};
use crate::layout::{Dim, Spec};
use crate::list::{List, Slot};
use crate::listview::{list_edit, mount_row, patch_list, ElReg, ElRows, LList, LRow, LRowMountFn, SrcParts};
use crate::rt::{PatchFn, Recipe, Rt};
use bough_core::RecordInit;
use std::any::TypeId;
use std::marker::PhantomData;

/// What a list child's node holds (`DNode::text`, in the bump store).
#[derive(Clone, Copy)]
pub(crate) struct ListReq {
    /// The `List<T>`.
    pub list: *const u8,
    /// The box the list is a child of (its gap is the rows' gap).
    pub parent: *const DNode,
    pub vt: &'static ListVt,
    /// The projection (`ViewRow`, or the function given to `each`).
    pub recipe: Recipe,
}

/// The functions of one list child, monomorphised for the item type and the projection.
pub(crate) struct ListVt {
    pub cell: fn() -> TypeId,
    pub list_name: fn() -> &'static str,
    pub item_ty: fn() -> TypeId,
    pub item_name: fn() -> &'static str,
    pub size: usize,
    pub len: unsafe fn(*const u8) -> usize,
    pub slots: unsafe fn(*const u8) -> usize,
    pub head: unsafe fn(*const u8) -> *const std::cell::Cell<u32>,
    pub mount: LRowMountFn,
    pub redescribe: unsafe fn(&mut Rt, usize, u32),
    pub slot_at: unsafe fn(*const u8, usize) -> u32,
    pub item: unsafe fn(*const u8, u32) -> *const u8,
    pub set_wired: unsafe fn(*const u8, &[u32]),
    pub edit: unsafe fn(*mut u8, u32, u32) -> *mut u8,
    pub gen_of: unsafe fn(*const u8, u32) -> u32,
    pub log_at: unsafe fn(*const u8) -> (u32, u32),
    pub patch: PatchFn,
}

/// A row description: `T::view` (the list of views itself), or the function given to [`List::each`].
pub trait Project<T, M>: Copy + 'static {
    #[doc(hidden)]
    fn project<'a>(&self, t: &'a T, s: Slot<T>) -> El<'a>;
    /// The row's `changed` (a list of views: the item's own).
    #[doc(hidden)]
    fn changed() -> Option<crate::rt::ChangedFn> {
        None
    }
}
impl<T: 'static, F: for<'a> Fn(&'a T) -> El<'a> + Copy + 'static> Project<T, fn(&T)> for F {
    #[inline(always)]
    fn project<'a>(&self, t: &'a T, _: Slot<T>) -> El<'a> {
        self(t)
    }
}
impl<T: 'static, F: for<'a> Fn(&'a T, Slot<T>) -> El<'a> + Copy + 'static> Project<T, fn(&T, Slot<T>)> for F {
    #[inline(always)]
    fn project<'a>(&self, t: &'a T, s: Slot<T>) -> El<'a> {
        self(t, s)
    }
}

/// The row of a list of views: the item's own `view`.
pub struct ViewRow<T>(PhantomData<fn() -> T>);
impl<T> Clone for ViewRow<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for ViewRow<T> {}
impl<T: View> Project<T, ()> for ViewRow<T> {
    #[inline(always)]
    fn project<'a>(&self, t: &'a T, _: Slot<T>) -> El<'a> {
        t.view()
    }
    fn changed() -> Option<crate::rt::ChangedFn> {
        Some(crate::changed::changed_shim::<T>)
    }
}

/// `list.each(f)`: the list's rows described by `f` (02 §4.1).
#[must_use = "a projection does nothing until it is placed in a container"]
pub struct ListEach<'f, T, F, M> {
    list: &'f List<T>,
    f: F,
    _m: PhantomData<fn() -> M>,
}

impl<T: 'static> List<T> {
    /// The list as children described by `f` (`column(self.tiles.each(Tile::compact))`): a method of the item
    /// (`fn(&T) -> El<'_>`), or one that also takes the row's slot (`fn(&T, Slot<T>) -> El<'_>`), for a row that
    /// binds slot-carrying routes. Rows are keyed by slot, as `&list` (a list of views) is.
    pub fn each<M, F: Project<T, M>>(&self, f: F) -> ListEach<'_, T, F, M> {
        ListEach { list: self, f, _m: PhantomData }
    }
}

struct Vt<T, P, M>(PhantomData<(fn() -> T, P, fn() -> M)>);
impl<T: 'static, P: Project<T, M>, M: 'static> Vt<T, P, M> {
    const VT: &'static ListVt = &ListVt {
        cell: crate::reflect::cell_id::<List<T>>,
        list_name: std::any::type_name::<List<T>>,
        item_ty: TypeId::of::<T>,
        item_name: std::any::type_name::<T>,
        size: size_of::<T>(),
        len: list_len::<T>,
        slots: list_slots::<T>,
        head: list_head::<T>,
        mount: mount_elrow::<T, P, M>,
        redescribe: redescribe_elrow::<T, P, M>,
        slot_at: crate::listview::list_slot_at::<T>,
        item: crate::listview::list_item::<T>,
        set_wired: crate::listview::list_set_wired::<T>,
        edit: list_edit::<T>,
        gen_of: crate::listview::list_gen::<T>,
        log_at: list_log_at::<T>,
        patch: patch_list::<T>,
    };
}

/// # Safety
/// `l` points at a live `List<T>`.
unsafe fn list_len<T>(l: *const u8) -> usize {
    // SAFETY: the function contract.
    unsafe { (*(l as *const List<T>)).len() }
}
/// # Safety
/// As `list_len`.
unsafe fn list_slots<T>(l: *const u8) -> usize {
    // SAFETY: the function contract.
    unsafe { (*(l as *const List<T>)).order_slots().iter().copied().max().map_or(0, |x| x as usize + 1) }
}
/// # Safety
/// As `list_len`.
unsafe fn list_log_at<T>(l: *const u8) -> (u32, u32) {
    // SAFETY: the function contract.
    let l = unsafe { &*(l as *const List<T>) };
    (l.epoch.get(), l.log().0.len() as u32)
}
/// # Safety
/// As `list_len`.
unsafe fn list_head<T>(l: *const u8) -> *const std::cell::Cell<u32> {
    // SAFETY: the function contract.
    unsafe { (*(l as *const List<T>)).head() }
}

#[inline]
fn put<'f, T: 'static, P: Project<T, M>, M: 'static>(parent: &mut DNode, list: &'f List<T>, p: P) {
    const { assert!(size_of::<P>() <= 16, "a row projection captures at most 16 bytes: name a method of the item") };
    let e = El::new(K_LIST, Spec::new(false, false));
    let n = e.fresh();
    n.text = put_req(ListReq { list: list as *const List<T> as *const u8, parent, vt: Vt::<T, P, M>::VT, recipe: Recipe::of(p) }) as *const u8;
    super::append(parent, n);
}
#[inline(always)]
fn put_req(q: ListReq) -> *mut ListReq {
    super::arena::put(q)
}

/// A list of views as children: each row is the item's `view`, keyed by its slot.
impl<'f, T: View> ChildSeq<'f> for &'f List<T> {
    #[inline]
    #[allow(private_interfaces)]
    fn into_children(self, parent: &mut DNode, _: super::plug::Seal) {
        put::<T, ViewRow<T>, ()>(parent, self, ViewRow(PhantomData));
    }
}
impl<'f, T: 'static, F: Project<T, M>, M: 'static> ChildSeq<'f> for ListEach<'f, T, F, M> {
    #[inline]
    #[allow(private_interfaces)]
    fn into_children(self, parent: &mut DNode, _: super::plug::Seal) {
        put::<T, F, M>(parent, self.list, self.f);
    }
}
/// A `Vec` of views as children: ordinal (02 §4 table). Each item's `view` is one item of a dynamic part that
/// realigns by position: a reorder re-describes positions and what the kernel keeps per position (hover, focus)
/// stays with the position, not the item. Keyed survival is what `List<T>` is for.
impl<'f, T: View> ChildSeq<'f> for &'f Vec<T> {
    #[inline]
    #[allow(private_interfaces)]
    fn into_children(self, parent: &mut DNode, _: super::plug::Seal) {
        super::group(parent, self.iter().map(View::view));
    }
}

/// The list request of a list node.
#[inline(always)]
pub(crate) fn req(n: &DNode) -> &ListReq {
    // SAFETY: a list node's `text` is the `ListReq` `put` wrote in this description's bump store.
    unsafe { &*(n.text as *const ListReq) }
}

/// What a re-description cannot change in place: the list, the projection, the gap.
pub(crate) fn shape(h: u64, n: &DNode, mix: impl Fn(u64, u64) -> u64) -> u64 {
    let q = req(n);
    let b = q.recipe.bytes();
    let gap = parent_spec(q).map_or(0, |s| s.gap.to_bits() as u64);
    let h = mix(mix(h, q.list as usize as u64), q.vt as *const ListVt as usize as u64);
    mix(mix(mix(h, u64::from_ne_bytes(b[..8].try_into().unwrap())), u64::from_ne_bytes(b[8..].try_into().unwrap())), gap)
}

fn parent_spec(q: &ListReq) -> Option<&Spec> {
    // SAFETY: the parent is a node of the same description (`put`), alive while it is walked.
    let p = unsafe { &*q.parent };
    (p.kind == K_BOX).then_some(&p.spec)
}

#[cold]
#[inline(never)]
#[track_caller]
fn not_a_field(list: &str, owner: &str) -> ! {
    panic!("`{list}` is a child of a view of `{owner}` but is not a field of it: a list's rows are keyed by its slots and follow its own log, so the list must be a field of the view that describes it (rows by position: `list.iter().map(..)`)")
}

/// Mount a list child: the region, its rows mounted at placement by `mount_elrow`.
pub(crate) fn mount<V: 'static>(m: &mut Mounter<'_, V>, n: &DNode) {
    let q = *req(n);
    let vt = q.vt;
    let Some(pspec) = parent_spec(&q).copied() else {
        panic!("`{}` is placed as a child of a box (`column(&self.items)`), not inside a tuple item, an `Option` or an iterator", (vt.list_name)())
    };
    if !pspec.column {
        panic!("`{}` in a row: a list region stacks its rows top to bottom; put it in a `column`", (vt.list_name)());
    }
    let (at, base) = (q.list as usize, m.owner as usize);
    let off = at.wrapping_sub(base);
    let ok = at >= base && off < size_of::<V>() && crate::reflect::cells::<V>().has(off, (vt.cell)());
    if !ok {
        not_a_field((vt.list_name)(), std::any::type_name::<V>());
    }
    // SAFETY: `q.list` is a live `List` of the item type `vt` was monomorphised for (`put`), proven a field of
    // the live owner above.
    let parts = unsafe {
        SrcParts {
            list: q.list,
            n: (vt.len)(q.list),
            slots: (vt.slots)(q.list),
            mount: vt.mount,
            slot_at: vt.slot_at,
            item: vt.item,
            set_wired: vt.set_wired,
            row_fn: 0,
            row_size: vt.size,
            item_ty: (vt.item_ty)(),
            item_name: (vt.item_name)(),
            edit: vt.edit,
            gen_of: vt.gen_of,
            el: Some(ElReg { recipe: q.recipe, redescribe: vt.redescribe, taped: m.taping() }),
            log_at: (vt.log_at)(q.list),
        }
    };
    // SAFETY: as above.
    let head = unsafe { (vt.head)(q.list) };
    let mut l = LList::new(ElRows { parts, head, off: off as u32, patch: vt.patch });
    l.spec = Spec::new(false, true);
    l.spec.w = Dim::Pct(1.0);
    l.spec.gap = pspec.gap;
    l.mount(m);
}

/// A re-description of the list's owner: the rows are the list's (state, not description), and their records
/// are stepped over in one step (the region keeps their count).
pub(crate) fn update<V>(m: &mut Mounter<'_, V>) {
    let reg = m.tape_next() as usize;
    let r = &mut m.rt.lists.regs[reg];
    debug_assert_eq!(r.recs, r.rows.iter().map(|x| x.nrec as usize).sum::<usize>(), "a list region's record count");
    r.seen = r.recs;
    m.pos += 1 + r.recs;
    m.kid += 1;
}

/// The El row mounter: the item of `slot`, described by the region's projection, mounted as an owner of the
/// region's class, with a tape when the region re-describes.
///
/// # Safety
/// `l` is a live `List<T>` and `slot` a live slot of it; region `reg` was mounted by `mount` with this
/// monomorph's `ListVt` (its recipe holds a `P`).
unsafe fn mount_elrow<T: 'static, P: Project<T, M>, M: 'static>(rt: &mut Rt, reg: u32, l: *const u8, slot: u32, recs: &mut Vec<RecordInit>) -> LRow {
    let (cls, el) = {
        let r = &rt.lists.regs[reg as usize];
        (r.cls, r.el.expect("an El list region"))
    };
    let list = l as *const List<T>;
    // (mutant: the slot id taken for a position: a row mounted after a reorder shows another item)
    let slot_m = if cfg!(feature = "mutant_el_list_slot") {
        // SAFETY: the function contract (a live list).
        unsafe { (*list).order_slots().get(slot as usize).copied().unwrap_or(slot) }
    } else {
        slot
    };
    // SAFETY: the function contract: a live slot holds an initialised `T`, the row's owner (class `cls`).
    let row = unsafe { crate::listview::list_item::<T>(l, slot_m) } as *const T;
    let s = unsafe { (*list).slot_of(slot_m) };
    // SAFETY: the region's recipe holds the `P` `put` wrote.
    let p: P = unsafe { el.recipe.read_at(0) };
    let mut tape = if el.taped { rt.lists.tape_pool.pop().unwrap_or_default() } else { Vec::new() };
    let tp: *mut Vec<u32> = if el.taped { &mut tape } else { std::ptr::null_mut() };
    let before = recs.len();
    // SAFETY: `row` is the live owner of class `cls`; `tp` is null or `tape`, alive for the mount.
    let lrow = unsafe {
        mount_row(rt, reg, row, cls, recs, tp, |m| {
            set_changed::<T, P, M>(m);
            let _walk = super::Walk::enter();
            p.project(&*row, s).mount(m)
        })
    };
    rt.lists.regs[reg as usize].stats.rows_described += 1;
    // the first record carries a handle: a re-description finds the row's records by it
    let first = if recs.len() > before { first_handle(rt, recs, before) } else { bough_core::Handle::default() };
    let si = lrow.scope as usize;
    let er = &mut rt.lists.elrows;
    if er.len() <= si {
        er.resize_with(si + 1, Default::default);
    }
    er[si].first = first;
    er[si].tape = tape;
    lrow
}

/// A row of a list of views is an owner: its `changed` runs when one of its boundary cells publishes.
fn set_changed<T: 'static, P: Project<T, M>, M>(m: &mut Mounter<'_, T>) {
    if let Some(f) = P::changed() {
        if m.rt.class_changed[m.cls as usize].is_none() {
            m.rt.class_changed[m.cls as usize] = Some(f);
        }
    }
}

/// The handle of record `at` of a mount batch (made if it has none), as `input::act_for` finds it.
fn first_handle(rt: &mut Rt, recs: &mut [RecordInit], at: usize) -> bough_core::Handle {
    let h = recs[at].handle;
    if rt.node_pending(h) {
        rt.handle(h)
    } else if h != 0 {
        rt.bough.handle_gen(h)
    } else {
        let n = rt.bough.new_handle();
        recs[at].handle = n.idx;
        n
    }
}

/// Re-describe the row of `slot` (its item was edited).
///
/// # Safety
/// Region `reg` was mounted by `mount` with this monomorph's `ListVt`; its list is live and `slot` live in it.
unsafe fn redescribe_elrow<T: 'static, P: Project<T, M>, M: 'static>(rt: &mut Rt, reg: usize, slot: u32) {
    let (l, el) = {
        let r = &rt.lists.regs[reg];
        (r.list, r.el.expect("an El list region"))
    };
    // SAFETY: the region's recipe holds the `P` `put` wrote; the list is live.
    let p: P = unsafe { el.recipe.read_at(0) };
    let s = unsafe { (*(l as *const List<T>)).slot_of(slot) };
    // SAFETY: the function contract.
    unsafe { rt.lr_redescribe::<T>(reg, l, slot, |t, m| p.project(t, s).update(m)) };
}
