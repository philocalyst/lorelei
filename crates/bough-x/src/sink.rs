//! Description by continuation (ROUND8 section 5) and lists as relations (section 3).
//!
//! `view` returns nothing. It drives a [`Sink`]: the same code mounts (`Write`, fresh), re-describes by comparing
//! against the columns (`Write`, not fresh), brings lists up to date after a scroll or an edit while skipping
//! everything fixed (`Patch`), and paints directly as an oracle (`soft::Immediate`).
//!
//! What is described is always a [`Row`]: a plain value (a record, a tuple or an array of rows) whose *type* is
//! the shape. The fixed part of a view is one row; a list's row is a pure function from an index to a row. A
//! row carries operands only. Where its records are is the layout's answer, given once per shape as a slice of
//! rects, so rows of a list that share a shape share one layout.
use crate::expr::U;
use crate::store::{Arch, Backend, Hov, Pass, Rect, Site, Space, Store, Tiles, desc};
use core::any::TypeId;
use core::marker::PhantomData;
use core::sync::atomic::{AtomicU32, Ordering};

pub trait View: Sized + 'static {
    /// The Frame-stage state: plain words the handlers write.
    fn block(&self) -> &U;
    /// The block for a pass. A view that declares its block hands it over; a view whose Frame state is its own
    /// fields (the element layer) has them gathered into `out` by the table of `slots` the world learned from
    /// its columns.
    #[inline(always)]
    fn gathered<'a>(&'a self, _slots: &[crate::expr::Slot], _out: &'a mut U) -> &'a U {
        self.block()
    }
    /// The same words, for the core to write the ones a space reads as its scroll offset (a wheel over a space
    /// scrolls it with no handler). A view that does not give them out scrolls only through its own handlers.
    fn block_mut(&mut self) -> Option<&mut U> {
        None
    }
    fn view<S: Sink<Self>>(&self, s: &mut S);
}

/// The sink a scope's body is given, for a borrow of `'r`.
///
/// This is a lifetime-generic associated type spelled as a supertrait, because the direct spelling
/// (`type Scope<'r>: Sink<V> where Self: 'r`) cannot be named under `for<'r>`: the higher-ranked bound ranges
/// over every `'r`, `'static` included, and `Self: 'r` then fails for any sink that borrows. The defaulted
/// `Implied = &'r Self` parameter carries `Self: 'r` as an implied bound, so `for<'r> ScopeOf<'r, V>` ranges
/// only over the lifetimes for which the sink is alive.
pub trait ScopeOf<'r, V, Implied = &'r Self> {
    type Scope: Sink<V>;
}

/// What a pass lays elements out against.
#[derive(Clone, Copy)]
pub struct Env<'a> {
    /// Device pixels per point.
    pub scale: f32,
    /// The window, in device pixels.
    pub size: [f32; 2],
    pub u: &'a U,
}

/// Where a list of elements is: its origin in the enclosing space and its width, in device pixels.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Geo {
    pub x: i32,
    pub y: i32,
    pub w: i32,
}

pub trait Sink<V>: Sized + for<'r> ScopeOf<'r, V> {
    /// Whether this sink takes the fixed rows at all. A pass after a turn does not: an element then skips its
    /// layout and its records and only reaches its lists.
    const FIXED: bool = true;
    /// What elements are laid out against. A sink that only takes rows with given rects need not answer.
    fn env(&self) -> Env<'_> {
        unimplemented!("this sink does not lay elements out")
    }
    /// The rows of a list of elements, one under another: `f(r)` is row `r`, an element that is its records
    /// and its own layout. `geo` is where the list is; a sink that keeps what it was told (`FIXED == false`)
    /// is given `None`. Returns the height of the list in device pixels.
    #[track_caller]
    fn rows<R: Row<V> + bough_lay::Shape, F: FnMut(usize) -> R>(
        &mut self,
        _n: u32,
        _geo: Option<Geo>,
        _f: F,
    ) -> i32 {
        unimplemented!("this sink does not take lists of elements")
    }
    /// As [`Sink::rows`], for rows that are a relation's: `edits` is its journal, and a pass describes the rows
    /// the journal names and the rows that enter exposure, not every exposed row. `how.all` asks for every
    /// exposed row to be described and compared anyway (a pass after which nothing kept from an earlier
    /// description may be relied on). `how.by_item` says that a row is described from its item alone, never
    /// from its index: then a row that an insert or a remove elsewhere moves is the same records at another
    /// place, and is not described again ([`RowsHow`]). A sink that keeps nothing between passes describes
    /// what it always does.
    #[track_caller]
    #[inline(always)]
    fn rows_j<R: Row<V> + bough_lay::Shape, F: FnMut(usize) -> R>(
        &mut self,
        n: u32,
        geo: Option<Geo>,
        _edits: &Journal,
        _how: RowsHow,
        f: F,
    ) -> i32 {
        self.rows(n, geo, f)
    }
    /// A row of the fixed shape: its layout (one rect per record, in order) and its operands. The closure is
    /// only called by a sink that wants the row; after a turn that changed no opaque state, none does.
    ///
    /// Every method is `#[track_caller]`: the reflecting sink records where in the view a row, a scope or a list
    /// is described; every other sink ignores the location, and after inlining it is not there.
    #[track_caller]
    fn row<R: Row<V>, L: AsRef<[Rect]>, F: FnOnce() -> (L, R)>(&mut self, f: F);
    /// Whether this sink holds the fixed row of this scope from an earlier pass, so that one record of it can
    /// be brought up to date alone ([`Sink::leaf`]) instead of the whole row being described.
    #[inline(always)]
    fn keeps(&self) -> bool {
        false
    }
    /// Record `k` of this scope's first fixed row is `a` now: stored and damaged if it differs from what the
    /// row holds, exactly as [`Sink::row`] would do for that record; the row's other records and every rect
    /// are left as they are. Only for a sink that [`Sink::keeps`]. The record's type is checked against the
    /// column the row was made with.
    fn leaf<A: Arch>(&mut self, _k: usize, _a: &A) {
        unimplemented!("this sink does not keep rows")
    }
    /// A scrolled scope's optional projected field and measured content height in device pixels.
    /// `line` overrides the first list row's default wheel distance, in points.
    fn scroll_content(
        &mut self,
        _field: Option<crate::expr::Slot>,
        _height: i32,
        _line: Option<f32>,
    ) {
    }

    /// A child with its own coordinate space (a clip, a scroll offset read from the block).
    #[track_caller]
    fn scope<F: for<'r> FnOnce(&mut <Self as ScopeOf<'r, V>>::Scope)>(
        &mut self,
        space: Space,
        f: F,
    );
    /// The rows of a relation, one under another. Only exposed rows are asked for.
    #[track_caller]
    fn list<R: Row<V>, F: FnMut(usize) -> R>(&mut self, l: &List<'_>, f: F);
    /// Free rows: `n` rows of one record each, every one with its own rect (`f(i)` is row `i` and where it is,
    /// in the space of this sink). Rows come and go between passes: a pass with fewer rows releases the rest, a
    /// pass with more writes them, and a row whose record or rect differs from what its slot holds is stored
    /// and damaged. Rows are painted in index order. This is where a description that is data lives
    /// ([`free_sync`]).
    #[track_caller]
    fn free<R: Row<V>, F: FnMut(usize) -> (Rect, R)>(&mut self, _n: u32, _f: F) {
        unimplemented!("this sink does not take free rows")
    }
}

/// How the rows of a relation are to be brought up to date ([`Sink::rows_j`]).
#[derive(Clone, Copy, Default, Debug)]
pub struct RowsHow {
    /// Describe and compare every exposed row, whatever the journal says.
    pub all: bool,
    /// A row's description depends on its item and not on its index (its handlers name the item, not the
    /// row; nothing it shows is the row's number). Only then may a row be kept across an insert or a remove
    /// before it: a list whose row function reads the index (`Sink::list`, `Sink::rows`) describes every row
    /// from the edit on again, because each of them is another description.
    pub by_item: bool,
}

// ---- relations ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Edit {
    Update(u32),
    /// Rows from here on are other rows than before (for a list: as an insert or a remove does).
    Shift(u32),
    /// A row was inserted at this index: the rows from here on moved down by one.
    Insert(u32),
    /// The row at this index was removed: the rows after it moved up by one.
    Remove(u32),
}

/// The last edits of a relation: its derivative. A list compares its own `seen` with `ver` and replays what it
/// missed; nothing has to be cleared, and any number of lists may read one relation.
pub struct Journal {
    watch: std::sync::OnceLock<std::sync::Arc<AtomicU32>>,
    pub ver: u32,
    pub log: [Edit; 8],
    /// The rows the list reading this journal exposed at its last pass (`lo << 32 | hi`): what whoever keeps
    /// the relation needs to turn "this item changed" into "this exposed row changed" without searching the
    /// whole order. Written by the list pass; one list per journal is assumed by its reader.
    pub win: core::sync::atomic::AtomicU64,
    /// The rows of this relation are described from their items alone, never from their index
    /// ([`RowsHow::by_item`]): a list reading this journal keeps a row across an insert or a remove before it.
    /// False unless the relation's owner says so ([`Journal::by_item`], [`Rel::by_item`]).
    pub by_item: bool,
}
impl Journal {
    pub const fn new() -> Journal {
        Journal {
            watch: std::sync::OnceLock::new(),
            ver: 0,
            log: [Edit::Update(0); 8],
            win: core::sync::atomic::AtomicU64::new(0),
            by_item: false,
        }
    }
    /// A journal of a relation whose rows are described from their items alone (see the field).
    pub const fn by_item() -> Journal {
        let mut j = Journal::new();
        j.by_item = true;
        j
    }
    fn watch(&self) -> std::sync::Arc<AtomicU32> {
        self.watch
            .get_or_init(|| std::sync::Arc::new(AtomicU32::new(self.ver)))
            .clone()
    }
    #[inline]
    fn follows(&self, current: &Option<std::sync::Arc<AtomicU32>>) -> bool {
        match (self.watch.get(), current.as_ref()) {
            (Some(watch), Some(current)) => std::sync::Arc::ptr_eq(watch, current),
            _ => false,
        }
    }
    // Once a list is following this journal, its Arc already owns the revision counter.
    // Rebind only when the source journal changes.
    fn follow(&self, current: &mut Option<std::sync::Arc<AtomicU32>>) -> bool {
        if self.follows(current) {
            return false;
        }
        *current = Some(self.watch());
        true
    }
    #[inline(always)]
    pub fn push(&mut self, e: Edit) {
        self.log[self.ver as usize & 7] = e;
        self.ver = self.ver.wrapping_add(1);
        if let Some(watch) = self.watch.get() {
            watch.store(self.ver, Ordering::Relaxed);
        }
    }
    /// The rows that changed since version `seen`: every row from `.0` on, and the `.2` rows listed in `.1`.
    /// The window the reading list exposed at its last pass.
    #[inline(always)]
    pub fn window(&self) -> (u32, u32) {
        let w = self.win.load(Ordering::Relaxed);
        ((w >> 32) as u32, w as u32)
    }
    #[inline]
    pub fn since(&self, seen: u32) -> (u32, [u32; 8], usize) {
        let pending = self.ver.wrapping_sub(seen);
        let (mut from, mut upd, mut n) = (u32::MAX, [0u32; 8], 0);
        if pending > 8 {
            return (0, upd, 0);
        }
        for k in 0..pending {
            match self.log[self.ver.wrapping_sub(1 + k) as usize & 7] {
                Edit::Update(at) => {
                    upd[n] = at;
                    n += 1;
                }
                Edit::Shift(at) | Edit::Insert(at) | Edit::Remove(at) => from = from.min(at),
            }
        }
        (from, upd, n)
    }
    /// The inserts and removes since version `seen`, oldest first, into `out` (what a reference to a row by its
    /// index is remapped by). `false` when more happened than the journal keeps: the rows' identities are lost.
    pub fn moves_since(&self, seen: u32, out: &mut Vec<Edit>) -> bool {
        out.clear();
        let pending = self.ver.wrapping_sub(seen);
        if pending > 8 {
            return false;
        }
        for k in (0..pending).rev() {
            match self.log[self.ver.wrapping_sub(1 + k) as usize & 7] {
                Edit::Update(_) => {}
                Edit::Shift(_) => return false,
                e => out.push(e),
            }
        }
        true
    }
}
impl Drop for Journal {
    fn drop(&mut self) {
        // A displayed relation may be replaced by assignment. The world's subscription still owns
        // this watch. Invalidate it so settle drives the view and binds the replacement journal.
        #[cfg(not(feature = "mutant_journal_drop"))]
        if let Some(watch) = self.watch.get() {
            watch.store(self.ver.wrapping_add(1), Ordering::Relaxed);
        }
    }
}
impl Default for Journal {
    fn default() -> Self {
        Journal::new()
    }
}
/// For a list over something that never changes.
pub static FIXED: Journal = Journal::new();

/// A collection whose changes are its own derivative: an edit is a plain call that also notes which row.
pub struct Rel<T> {
    items: Vec<T>,
    pub journal: Journal,
}
impl<T> Rel<T> {
    pub fn new(items: Vec<T>) -> Rel<T> {
        Rel {
            items,
            journal: Journal::new(),
        }
    }
    /// As [`Rel::new`], for rows described from their items alone ([`Journal::by_item`]): the view's row
    /// function reads `get(r)` and nothing else of `r`.
    pub fn by_item(items: Vec<T>) -> Rel<T> {
        Rel {
            items,
            journal: Journal::by_item(),
        }
    }
    #[inline(always)]
    pub fn len(&self) -> usize {
        self.items.len()
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    #[inline(always)]
    pub fn get(&self, i: usize) -> &T {
        &self.items[i]
    }
    pub fn items(&self) -> &[T] {
        &self.items
    }
    /// The items with no note taken: for state kept opaque, where the view is re-described and compared.
    pub fn items_mut(&mut self) -> &mut Vec<T> {
        &mut self.items
    }
    #[inline(always)]
    pub fn edit(&mut self, i: usize) -> &mut T {
        self.journal.push(Edit::Update(i as u32));
        &mut self.items[i]
    }
    pub fn insert(&mut self, i: usize, v: T) {
        self.journal.push(Edit::Insert(i as u32));
        self.items.insert(i, v);
    }
    pub fn remove(&mut self, i: usize) -> T {
        self.journal.push(Edit::Remove(i as u32));
        self.items.remove(i)
    }
}

/// A list of `n` rows of one height and one layout. `x`, `y` are in the enclosing space; `shape` is the layout
/// of one row, relative to the row.
pub struct List<'a> {
    pub n: u32,
    pub row_h: i32,
    pub x: i32,
    pub y: i32,
    pub shape: &'a [Rect],
    pub edits: &'a Journal,
}

// ---- rows --------------------------------------------------------------------------------------------------

pub trait Leaf<V> {
    fn leaf<A: Arch>(&mut self, a: &A, key: TypeId, hov: Option<Hov>);
    /// The next leaf was written here (the element layer's own call site). Only a sink that records
    /// locations keeps it; every other sink ignores it, and after inlining it is not there.
    #[cfg(feature = "inspect-metadata")]
    #[inline(always)]
    fn at(&mut self, _loc: &'static core::panic::Location<'static>) {}
    /// A place in a row that is not a record: a child with its own space (a scrolled container), whose rect
    /// the row's layout gives. It takes one position of the row so layout and records count alike.
    #[inline(always)]
    fn hole(&mut self) {}
}

/// `TypeId` of a type that may borrow: the identity of the type with its lifetimes erased. A row may hold
/// closures that borrow the view; which columns it is written to depends on its type and never on a lifetime.
#[inline(always)]
pub fn tid<T: ?Sized>() -> TypeId {
    trait Key {
        fn key(&self) -> TypeId
        where
            Self: 'static;
    }
    impl<T: ?Sized> Key for PhantomData<T> {
        #[inline(always)]
        fn key(&self) -> TypeId
        where
            Self: 'static,
        {
            TypeId::of::<T>()
        }
    }
    let p = PhantomData::<T>;
    // SAFETY: only the vtable's `key` is called, which reads nothing of the value; extending the trait
    // object's lifetime bound cannot let a borrow escape because no data of `T` exists in a `PhantomData`.
    unsafe { core::mem::transmute::<&dyn Key, &(dyn Key + 'static)>(&p).key() }
}

/// The shape of a row: a record, a tuple of rows, or an array of rows. Its type fixes the sequence of
/// archetypes, so the columns of a store are resolved once, when the row is first described.
///
/// # Safety
/// The sinks and the composite rows (tuples, arrays, boxes) find a row's records by position: element `k`
/// of a tuple is written at the sites after the `LEAVES` of the elements before it, and those sites were
/// made by `visit`. An implementation therefore promises (soundness review S05):
/// * `visit` reports exactly `LEAVES` leaves and holes, and the sequence of their archetypes, keys and
///   handler faces depends on the type alone, never on the value;
/// * `write` and `patch` touch only the `LEAVES` sites from `sites`, writing at each the record type its
///   `visit` reported there.
///
/// A safe implementation whose `LEAVES` disagreed with `visit` used to shift a built-in record onto another
/// archetype's column:
///
/// ```compile_fail,E0200
/// use bough_x::prelude::*;
/// use bough_x::sink::{Cx, Leaf, Mk};
/// use bough_x::store::Site;
/// struct Liar;
/// impl<V> Row<V> for Liar {
///     const LEAVES: u32 = 1;
///     fn visit<L: Leaf<V>>(&self, _: &mut L) {}
///     unsafe fn write(&self, _: *const Site, _: u32) {}
///     unsafe fn patch(&self, _: *const Site, _: u32, _: &Mk, _: &mut Cx<'_>) {}
/// }
/// ```
pub unsafe trait Row<V> {
    const LEAVES: u32;
    fn visit<L: Leaf<V>>(&self, l: &mut L);
    /// Write the row's operands into slot `slot`.
    ///
    /// SAFETY: `sites` points at `LEAVES` sites discovered from a value of this type, whose pointers are
    /// current, and `slot` is under the store's capacity.
    unsafe fn write(&self, sites: *const Site, slot: u32);
    /// Compare with what slot `slot` holds; store and damage what differs.
    ///
    /// SAFETY: as [`Row::write`].
    unsafe fn patch(&self, sites: *const Site, slot: u32, mk: &Mk, cx: &mut Cx<'_>);
    /// Patch a row whose non-hole records are already initialized.
    ///
    /// SAFETY: as `patch`, and every non-hole site contains its initialized record at `slot`.
    #[inline(always)]
    unsafe fn patch_initialized(&self, sites: *const Site, slot: u32, mk: &Mk, cx: &mut Cx<'_>) {
        // SAFETY: the precondition includes the ordinary patch contract.
        unsafe { self.patch(sites, slot, mk, cx) }
    }
}

/// Where a store's records are on screen during a pass.
#[derive(Clone, Copy)]
pub struct Mk {
    pub org: (i32, i32),
    pub clip: Rect,
}

// SAFETY: one leaf, of archetype `A`, written as an `A` at the one site.
unsafe impl<V, A: Arch> Row<V> for A {
    const LEAVES: u32 = 1;
    #[inline(always)]
    fn visit<L: Leaf<V>>(&self, l: &mut L) {
        l.leaf(self, TypeId::of::<A>(), None)
    }
    #[inline(always)]
    unsafe fn write(&self, sites: *const Site, slot: u32) {
        // SAFETY: the caller's contract: the site's column is of archetype `A` and holds `slot`.
        unsafe { write_arch_record(&*sites, slot, *self) }
    }
    #[inline(always)]
    unsafe fn patch(&self, sites: *const Site, slot: u32, mk: &Mk, cx: &mut Cx<'_>) {
        // SAFETY: as `write`.
        unsafe {
            let Some(p) = (*sites).initialized_record::<A>(slot) else {
                write_missing(&*sites, slot, *self, mk, cx);
                return;
            };
            patch_record(&*sites, slot, p, *self, mk, cx);
        }
    }
    #[inline(always)]
    unsafe fn patch_initialized(&self, sites: *const Site, slot: u32, mk: &Mk, cx: &mut Cx<'_>) {
        // SAFETY: the caller supplies an initialized slot of this record type.
        unsafe { patch_record(&*sites, slot, (*sites).rec::<A>(slot), *self, mk, cx) }
    }
}

/// SAFETY: `p` addresses the initialized record of archetype `A` at `slot` of `site`.
#[inline(always)]
unsafe fn patch_record<A: Arch>(
    site: &Site,
    slot: u32,
    p: *mut A,
    value: A,
    mk: &Mk,
    cx: &mut Cx<'_>,
) {
    // SAFETY: the caller supplies the record and its lane. Only typed values of `A` are written.
    unsafe {
        if A::STATE {
            if !(*p).eq_ops(&value) {
                let mut next = value;
                next.carry(&*p, cx.u, cx.u);
                replace(site, slot, p, next, mk, cx);
            }
        } else if !(*p).same_at(&value, cx.u) {
            replace(site, slot, p, value, mk, cx);
        } else {
            change_resource(Some(&*p), &value);
            p.write(value);
        }
    }
}

#[inline(always)]
fn change_resource<A: Arch>(old: Option<&A>, new: &A) {
    if A::OWNS_RESOURCE && !old.is_some_and(|old| old.same_resource(new)) {
        new.acquire_resource();
        if let Some(old) = old {
            old.release_resource();
        }
    }
}

/// SAFETY: the site holds a column of A, and slot is in capacity.
#[inline(always)]
unsafe fn write_arch_record<A: Arch>(site: &Site, slot: u32, value: A) {
    // SAFETY: the caller proves the slot, type and page. Read the old record only when initialized.
    unsafe {
        if A::OWNS_RESOURCE {
            let old = site.initialized_record::<A>(slot).map(|p| &*p);
            change_resource(old, &value);
        }
        site.write_record(slot, value);
    }
}

/// Initialize a record first reached during a patch, including a row left by an unwound view.
///
/// SAFETY: the site holds A, its pointers are current, and the slot is under capacity.
#[cold]
#[inline(never)]
unsafe fn write_missing<A: Arch>(site: &Site, slot: u32, value: A, mk: &Mk, cx: &mut Cx<'_>) {
    // SAFETY: the caller supplies the site, column type, and slot needed for a typed write.
    unsafe {
        let r = site.rect(slot).shift(mk.org);
        cx.tiles.mark(r.shift(value.offset(cx.u)).and(mk.clip));
        cx.tiles.mark(r.and(mk.clip));
        write_arch_record(site, slot, value);
    }
    cx.changed += 1;
}

/// A record whose operands differ from what the column holds: damage what it showed, store it, damage what it
/// shows (the same place unless its paint moves it).
///
/// SAFETY: `p` is the record of `site` at `slot`.
#[inline(never)]
unsafe fn replace<A: Arch>(site: &Site, slot: u32, p: *mut A, a: A, mk: &Mk, cx: &mut Cx<'_>) {
    // SAFETY: the caller's contract.
    unsafe {
        let r = site.rect(slot).shift(mk.org);
        if A::MOVES {
            cx.tiles.mark(r.shift((*p).offset(cx.u)).and(mk.clip));
            cx.tiles.mark(r.shift(a.offset(cx.u)).and(mk.clip));
        } else {
            cx.tiles.mark(r.and(mk.clip));
        }
        change_resource(Some(&*p), &a);
        *p = a;
        cx.changed += 1;
    }
}

// SAFETY: `N` elements, each of `X::LEAVES` sites, visited, written and patched in order.
unsafe impl<V, X: Row<V>, const N: usize> Row<V> for [X; N] {
    const LEAVES: u32 = X::LEAVES * N as u32;
    #[inline(always)]
    fn visit<L: Leaf<V>>(&self, l: &mut L) {
        for x in self {
            x.visit(l);
        }
    }
    #[inline(always)]
    unsafe fn write(&self, sites: *const Site, slot: u32) {
        for (k, x) in self.iter().enumerate() {
            // SAFETY: element `k` owns the `X::LEAVES` sites from `k * X::LEAVES`, part of the caller's range.
            unsafe { x.write(sites.add(k * X::LEAVES as usize), slot) }
        }
    }
    #[inline(always)]
    unsafe fn patch(&self, sites: *const Site, slot: u32, mk: &Mk, cx: &mut Cx<'_>) {
        for (k, x) in self.iter().enumerate() {
            // SAFETY: as `write`.
            unsafe { x.patch(sites.add(k * X::LEAVES as usize), slot, mk, cx) }
        }
    }
    #[inline(always)]
    unsafe fn patch_initialized(&self, sites: *const Site, slot: u32, mk: &Mk, cx: &mut Cx<'_>) {
        for (k, x) in self.iter().enumerate() {
            // SAFETY: the element owns those sites, all admitted by the caller.
            unsafe { x.patch_initialized(sites.add(k * X::LEAVES as usize), slot, mk, cx) }
        }
    }
}

macro_rules! row_tuple {
    ($($t:ident)+) => {
        // SAFETY: the elements in order, each at the sites after the `LEAVES` of those before it.
        unsafe impl<V, $($t: Row<V>),+> Row<V> for ($($t,)+) {
            const LEAVES: u32 = 0 $(+ $t::LEAVES)+;
            #[inline(always)]
            #[allow(non_snake_case)]
            fn visit<Lf: Leaf<V>>(&self, l: &mut Lf) {
                let ($($t,)+) = self;
                $($t.visit(l);)+
            }
            #[inline(always)]
            #[allow(non_snake_case, unused_assignments)]
            unsafe fn write(&self, sites: *const Site, slot: u32) {
                let ($($t,)+) = self;
                let mut p = sites;
                // SAFETY: each element owns the next `LEAVES` sites of the caller's range.
                $(unsafe { $t.write(p, slot); p = p.add($t::LEAVES as usize); })+
            }
            #[inline(always)]
            #[allow(non_snake_case, unused_assignments)]
            unsafe fn patch(&self, sites: *const Site, slot: u32, mk: &Mk, cx: &mut Cx<'_>) {
                let ($($t,)+) = self;
                let mut p = sites;
                // SAFETY: as `write`.
                $(unsafe { $t.patch(p, slot, mk, cx); p = p.add($t::LEAVES as usize); })+
            }
            #[inline(always)]
            #[allow(non_snake_case, unused_assignments)]
            unsafe fn patch_initialized(&self, sites: *const Site, slot: u32, mk: &Mk, cx: &mut Cx<'_>) {
                let ($($t,)+) = self;
                let mut p = sites;
                // SAFETY: each element receives its initialized sites.
                $(unsafe { $t.patch_initialized(p, slot, mk, cx); p = p.add($t::LEAVES as usize); })+
            }
        }
    };
}
row_tuple!(A);
row_tuple!(A B);
row_tuple!(A B C);
row_tuple!(A B C D);
row_tuple!(A B C D E);
row_tuple!(A B C D E F);
row_tuple!(A B C D E F G);
row_tuple!(A B C D E F G H);
row_tuple!(A B C D E F G H I);
row_tuple!(A B C D E F G H I J);
row_tuple!(A B C D E F G H I J K);
row_tuple!(A B C D E F G H I J K L);

// ---- the sinks ---------------------------------------------------------------------------------------------

/// What a pass carries: the damage, the block, whether every exposed row is to be compared, and (out) the
/// block words that move some list's exposure.
pub struct Cx<'c> {
    /// (out) A fixed record moved so that painting column by column no longer paints in the order described:
    /// the stores are to be made again.
    pub regroup: bool,
    pub tiles: &'c mut Tiles,
    pub u: &'c U,
    pub all: bool,
    pub lists: u64,
    /// Device pixels per point (what elements are laid out with).
    pub scale: f32,
    /// Records this pass stored again with other words or another rect, or lists it moved (an
    /// over-approximation): what a reader of the stores (reflection, accessibility) compares to know whether
    /// anything it read may have changed.
    pub changed: u32,
    /// The part of `changed` that is something being somewhere else (a rect, a row entering or leaving, a
    /// list's window or layout): what may have put other content under a still pointer. A record stored
    /// again with other operands where it was does not count.
    pub moved: u32,
}

struct Discover<'a, B> {
    st: &'a mut Store,
    rects: &'a [Rect],
    k: usize,
    _b: PhantomData<B>,
}
impl<V, B: Backend> Leaf<V> for Discover<'_, B> {
    fn leaf<A: Arch>(&mut self, _: &A, key: TypeId, hov: Option<Hov>) {
        self.st.discover(
            desc::<A, B>(),
            self.rects[self.k],
            key,
            TypeId::of::<A>(),
            hov,
        );
        self.k += 1;
    }
    fn hole(&mut self) {
        self.st.hole();
        self.k += 1;
    }
}

/// The sites of row `k` of the store's shape, discovering them the first time. One check per row value.
#[inline(always)]
fn row_sites<V, R: Row<V>, B: Backend>(st: &mut Store, k: usize, rects: &[Rect], row: &R) -> u32 {
    assert!(
        rects.len() == R::LEAVES as usize,
        "a row's layout has one rect per record"
    );
    if k == st.rows.len() {
        return row_new::<V, R, B>(st, rects, row);
    }
    let (ty, p0) = st.rows[k];
    assert!(
        ty == tid::<R>(),
        "the shape of a view changed between passes; a part that comes and goes is a list"
    );
    p0
}
#[cold]
#[inline(never)]
fn row_new<V, R: Row<V>, B: Backend>(st: &mut Store, rects: &[Rect], row: &R) -> u32 {
    let p0 = st.sites.len() as u32;
    row.visit(&mut Discover::<B> {
        st,
        rects,
        k: 0,
        _b: PhantomData,
    });
    st.rows.push((tid::<R>(), p0));
    p0
}

/// Mount and re-description: every row described is written (fresh) or compared with its columns.
pub struct Write<'w, 'c, V, B> {
    pub(crate) st: &'w mut Store,
    pub(crate) cx: &'w mut Cx<'c>,
    pub(crate) rowc: u32,
    pub(crate) sub: u32,
    pub(crate) fresh: bool,
    pub(crate) mk: Mk,
    pub(crate) _p: PhantomData<fn(&mut V, B)>,
}

impl<'w, 'c, V, B: Backend> Write<'w, 'c, V, B> {
    pub(crate) fn new(st: &'w mut Store, cx: &'w mut Cx<'c>, fresh: bool) -> Self {
        let mk = Mk {
            org: st.space.origin(cx.u),
            clip: st.space.clip,
        };
        Write {
            st,
            cx,
            rowc: 0,
            sub: 0,
            fresh,
            mk,
            _p: PhantomData,
        }
    }
}

impl<'r, 'w, 'c, V, B: Backend> ScopeOf<'r, V> for Write<'w, 'c, V, B> {
    type Scope = Write<'r, 'c, V, B>;
}
impl<'w, 'c, V, B: Backend> Sink<V> for Write<'w, 'c, V, B> {
    #[inline]
    fn row<R: Row<V>, L: AsRef<[Rect]>, F: FnOnce() -> (L, R)>(&mut self, f: F) {
        let (rects, row) = f();
        let rects = rects.as_ref();
        let k = self.rowc as usize;
        self.rowc += 1;
        let p0 = row_sites::<V, R, B>(self.st, k, rects, &row) as usize;
        let sites = &self.st.sites[p0..p0 + rects.len()];
        if self.fresh {
            for (s, r) in sites.iter().zip(rects) {
                if s.col == u32::MAX {
                    continue;
                }
                // SAFETY: slot 0 of a site of this store (capacity 1).
                unsafe { s.set_rect(0, *r) }
            }
            // SAFETY: the sites were discovered from a row of type `R` (`row_sites`).
            unsafe { row.write(sites.as_ptr(), 0) }
        } else {
            let mut moved = false;
            for (s, r) in sites.iter().zip(rects) {
                if s.col == u32::MAX {
                    continue;
                }
                // SAFETY: as above.
                unsafe {
                    let old = s.rect(0);
                    if old != *r {
                        // A fixed record somewhere else: rare (a resize in re-describing mode). Its paint may
                        // move it further, which this untyped path cannot see, so the whole space is damaged.
                        self.cx.tiles.mark(self.mk.clip);
                        self.cx.changed += 1;
                        self.cx.moved += 1;
                        s.set_rect(0, *r);
                        moved = true;
                    }
                }
            }
            // SAFETY: as above.
            unsafe { row.patch(sites.as_ptr(), 0, &self.mk, self.cx) }
            if moved && !self.st.order_holds() {
                self.cx.regroup = true;
            }
        }
    }
    #[inline(always)]
    fn keeps(&self) -> bool {
        !self.fresh && self.rowc == 0 && !self.st.rows.is_empty()
    }
    #[inline]
    fn leaf<A: Arch>(&mut self, k: usize, a: &A) {
        let p0 = self.st.rows[0].1 as usize;
        let end = self
            .st
            .rows
            .get(1)
            .map_or(self.st.sites.len(), |r| r.1 as usize);
        let site = self.st.sites[p0..end][k];
        assert!(
            site.col != u32::MAX && self.st.cols[site.col as usize].arch == TypeId::of::<A>(),
            "a record of another type than the row's column"
        );
        assert!(
            self.st.cols[site.col as usize].rec_init(site.base),
            "an unwritten leaf"
        );
        // SAFETY: the site is record `k` of this store's first row (in range, above), not a hole, and its
        // column was made for archetype `A` (checked above); slot 0 is the fixed row's slot.
        unsafe { <A as Row<V>>::patch(a, &site, 0, &self.mk, self.cx) }
    }
    fn scroll_content(&mut self, field: Option<crate::expr::Slot>, height: i32, line: Option<f32>) {
        if self.st.scroll_content.as_ref().and_then(|info| info.field) != field {
            self.st.bump_structure();
        }
        let new = crate::store::ScrollContent {
            field,
            height,
            line,
        };
        if let Some(info) = self.st.scroll_content.as_mut() {
            **info = new;
        } else {
            self.st.scroll_content = Some(Box::new(new));
        }
    }
    fn scope<F: for<'r> FnOnce(&mut <Self as ScopeOf<'r, V>>::Scope)>(
        &mut self,
        space: Space,
        f: F,
    ) {
        let q = self.sub as usize;
        self.sub += 1;
        let mut fresh = self.fresh;
        if q == self.st.subs.len() {
            let mut child = Store::new(1, space);
            child.structure = self.st.structure.clone();
            child.column_count = self.st.column_count.clone();
            self.st.subs.push(child);
            self.st.bump_structure();
            self.st.order.push(Pass::Sub(q as u32));
            fresh = true;
        }
        let st = &mut self.st.subs[q];
        if st.space != space {
            if !fresh {
                self.cx.tiles.mark(st.space.clip);
                self.cx.tiles.mark(space.clip);
            }
            st.set_space(space);
        }
        st.row_live[0] = 1;
        let mut sub = Write::<V, B>::new(st, self.cx, fresh);
        f(&mut sub);
        let st = &self.st.subs[q];
        if st.timed {
            st.resweep(self.cx.u, self.cx.tiles);
        }
    }
    #[inline(always)]
    fn list<R: Row<V>, F: FnMut(usize) -> R>(&mut self, l: &List<'_>, f: F) {
        let q = self.sub as usize;
        self.sub += 1;
        list_sync::<V, R, F, B>(self.st, q, self.cx, l, f, core::panic::Location::caller());
    }
    #[inline(always)]
    fn free<R: Row<V>, F: FnMut(usize) -> (Rect, R)>(&mut self, n: u32, f: F) {
        let q = self.sub as usize;
        self.sub += 1;
        free_sync::<V, R, F, B>(self.st, q, self.cx, n, f);
    }
    #[inline(always)]
    fn env(&self) -> Env<'_> {
        Env {
            scale: self.cx.scale,
            size: [self.cx.tiles.w as f32, self.cx.tiles.h as f32],
            u: self.cx.u,
        }
    }
    #[inline(always)]
    fn rows<R: Row<V> + bough_lay::Shape, F: FnMut(usize) -> R>(
        &mut self,
        n: u32,
        geo: Option<Geo>,
        f: F,
    ) -> i32 {
        let q = self.sub as usize;
        self.sub += 1;
        let all = self.cx.all;
        rows_sync::<V, R, F, B>(
            self.st,
            q,
            self.cx,
            n,
            geo,
            (
                &FIXED,
                RowsHow {
                    all,
                    by_item: false,
                },
            ),
            f,
            core::panic::Location::caller(),
        )
    }
    #[inline(always)]
    fn rows_j<R: Row<V> + bough_lay::Shape, F: FnMut(usize) -> R>(
        &mut self,
        n: u32,
        geo: Option<Geo>,
        edits: &Journal,
        how: RowsHow,
        f: F,
    ) -> i32 {
        let q = self.sub as usize;
        self.sub += 1;
        rows_sync::<V, R, F, B>(
            self.st,
            q,
            self.cx,
            n,
            geo,
            (edits, how),
            f,
            core::panic::Location::caller(),
        )
    }
}

/// After a turn: the fixed shape cannot have changed (its operands are expressions over the block), so its
/// rows are not even built; only lists are brought up to date.
pub struct Patch<'w, 'c, V, B> {
    pub(crate) st: &'w mut Store,
    pub(crate) cx: &'w mut Cx<'c>,
    pub(crate) sub: u32,
    pub(crate) _p: PhantomData<fn(&mut V, B)>,
}

impl<'r, 'w, 'c, V, B: Backend> ScopeOf<'r, V> for Patch<'w, 'c, V, B> {
    type Scope = Patch<'r, 'c, V, B>;
}
impl<'w, 'c, V, B: Backend> Sink<V> for Patch<'w, 'c, V, B> {
    const FIXED: bool = false;
    #[inline(always)]
    fn env(&self) -> Env<'_> {
        Env {
            scale: self.cx.scale,
            size: [self.cx.tiles.w as f32, self.cx.tiles.h as f32],
            u: self.cx.u,
        }
    }
    #[inline(always)]
    fn rows<R: Row<V> + bough_lay::Shape, F: FnMut(usize) -> R>(
        &mut self,
        n: u32,
        _: Option<Geo>,
        f: F,
    ) -> i32 {
        let q = self.sub as usize;
        self.sub += 1;
        let all = self.cx.all;
        rows_sync::<V, R, F, B>(
            self.st,
            q,
            self.cx,
            n,
            None,
            (
                &FIXED,
                RowsHow {
                    all,
                    by_item: false,
                },
            ),
            f,
            core::panic::Location::caller(),
        )
    }
    #[inline(always)]
    fn rows_j<R: Row<V> + bough_lay::Shape, F: FnMut(usize) -> R>(
        &mut self,
        n: u32,
        _: Option<Geo>,
        edits: &Journal,
        how: RowsHow,
        f: F,
    ) -> i32 {
        let q = self.sub as usize;
        self.sub += 1;
        rows_sync::<V, R, F, B>(
            self.st,
            q,
            self.cx,
            n,
            None,
            (edits, how),
            f,
            core::panic::Location::caller(),
        )
    }
    #[inline(always)]
    fn row<R: Row<V>, L: AsRef<[Rect]>, F: FnOnce() -> (L, R)>(&mut self, _: F) {}
    /// Free rows read no block word: a pass after a turn has nothing to bring up to date in them.
    #[inline(always)]
    fn free<R: Row<V>, F: FnMut(usize) -> (Rect, R)>(&mut self, _: u32, _: F) {
        self.sub += 1;
    }
    #[inline(always)]
    fn scope<F: for<'r> FnOnce(&mut <Self as ScopeOf<'r, V>>::Scope)>(&mut self, _: Space, f: F) {
        let q = self.sub as usize;
        self.sub += 1;
        let mut sub = Patch::<V, B> {
            st: &mut self.st.subs[q],
            cx: self.cx,
            sub: 0,
            _p: PhantomData,
        };
        f(&mut sub);
    }
    #[inline(always)]
    fn list<R: Row<V>, F: FnMut(usize) -> R>(&mut self, l: &List<'_>, f: F) {
        let q = self.sub as usize;
        self.sub += 1;
        list_sync::<V, R, F, B>(self.st, q, self.cx, l, f, core::panic::Location::caller());
    }
}

/// The exposed rows of a list of `n` rows under a space at `org` with clip `clip`: the rows of the list's
/// extent (bough-lay's [`Uniform`], 64-bit) that meet the clip, `(n, n)` past its end and `(0, 0)` before its
/// start. A list of rows with no height exposes nothing (it is refused where it is described: [`admit`]).
#[inline(always)]
pub fn exposed(l: &List<'_>, org: (i32, i32), clip: Rect) -> (u32, u32) {
    use bough_lay::Extent;
    let Some(ext) = bough_lay::Uniform::new(l.n, l.row_h) else {
        return (0, 0);
    };
    let origin = i64::from(org.1) + i64::from(l.y);
    let top = i64::from(clip.y0) - origin;
    let bottom = i64::from(clip.y1) - origin;
    if top >= ext.total() {
        return (l.n, l.n);
    }
    if bottom <= top || bottom <= 0 || l.n == 0 {
        return (0, 0);
    }
    (ext.row_at(top), ext.row_at(bottom - 1) + 1)
}

/// A list as described: rows of a positive height (review L03: a height of 0 divided where the list was
/// scrolled), at most 2^30 of them (a plan's row index keeps two flag bits), and a total height that a 32-bit
/// position holds (review C07). Anything else is refused here, with what was asked.
#[inline(always)]
pub fn admit(n: u32, row_h: i32) {
    assert!(
        row_h > 0 || n == 0,
        "a list's rows have a positive height (asked: {n} rows of {row_h} px)"
    );
    assert!(
        n < 1 << 30 && n as i64 * row_h.max(0) as i64 <= i32::MAX as i64,
        "a list holds under 2^30 rows and under 2^31 px (asked: {n} rows of {row_h} px)"
    );
}

/// Bring list `q` of `parent` up to date: rows that entered exposure are written, rows the relation's journal
/// names are compared, rows that left are released. Nothing else is touched, and nothing outside the window
/// exists.
///
/// What is generic here is only what must be: asking the row function for a row, and writing or comparing its
/// records. Which rows, their layout, their damage and the window are decided by [`list_plan`], one body for
/// every list in a program.
#[inline]
///
/// A list's rows are known to be the rows its columns were made for by where they are described (the list's call
/// site) and their shape (leaves and size), not by their Rust type: a closure inside `view` (a handler, a mapped
/// function) is a different type in each sink's instantiation of `view`, the same source and the same layout.
pub fn list_sync<V, R: Row<V>, F: FnMut(usize) -> R, B: Backend>(
    parent: &mut Store,
    q: usize,
    cx: &mut Cx<'_>,
    l: &List<'_>,
    mut f: F,
    at: &'static core::panic::Location<'static>,
) {
    admit(l.n, l.row_h);
    let space = parent.space;
    cx.lists |= space.reads();
    if q == parent.subs.len() {
        list_new(parent, l);
    }
    let st = &mut parent.subs[q];
    let org = space.origin(cx.u);
    let (lo, hi) = exposed(l, org, space.clip);
    if (lo == st.lo) & (hi == st.hi) & (st.seen == l.edits.ver) & !cx.all
        && l.edits.follows(&st.journal)
    {
        return;
    }
    if lo < hi
        && !st.rows.is_empty()
        && (hi - lo > st.cap || st.rows[0].0 != tid::<R>() || st.sites.len() != R::LEAVES as usize)
    {
        // A row of the discovered type has the same site layout by the unsafe Row contract. Another
        // sink monomorphization may produce a different type. Compare its layout before reuse.
        let mut same = Same {
            st: &*st,
            k: 0,
            ok: hi - lo <= st.cap,
        };
        f(lo as usize).visit(&mut same);
        if !(same.ok && same.k == st.sites.len()) {
            list_reset(st, cx, hi - lo);
        }
    }
    if st.rows.is_empty() && lo < hi {
        row_new::<V, R, B>(st, l.shape, &f(lo as usize));
        st.src = Some((at, R::LEAVES, size_of::<R>() as u32));
    }
    let mk = list_plan(
        st,
        cx,
        l,
        (lo, hi),
        org,
        space,
        R::LEAVES as usize,
        (at, R::LEAVES, size_of::<R>() as u32),
        RowsHow {
            all: cx.all,
            by_item: l.edits.by_item,
        },
    );
    let (sites, mask, bias) = (st.sites.as_ptr(), st.cap - 1, st.bias);
    let plan = core::mem::take(&mut st.plan);
    // The certificate is about physical slots. Rigid replay can change the bias without
    // changing the logical window.
    let window = (lo.wrapping_add(bias) & mask, hi - lo);
    if cx.all && st.initialized_window == Some(window) {
        for &e in &plan {
            let r = e & 0x3fff_ffff;
            let row = f(r as usize);
            let slot = r.wrapping_add(bias) & mask;
            // SAFETY: the admitted shape and certificate cover the current sites and slots.
            unsafe {
                if e >> 31 != 0 {
                    row.write(sites, slot);
                } else {
                    row.patch_initialized(sites, slot, &mk, cx);
                }
            }
        }
    } else {
        for &e in &plan {
            let r = e & 0x3fff_ffff;
            let row = f(r as usize);
            let slot = r.wrapping_add(bias) & mask;
            // SAFETY: the sites and slot belong to the admitted row layout.
            unsafe {
                if e >> 31 != 0 {
                    row.write(sites, slot);
                } else {
                    row.patch(sites, slot, &mk, cx);
                }
            }
        }
        if cx.all && lo < hi {
            st.initialized_window = st
                .sites
                .iter()
                .all(|site| {
                    site.col == u32::MAX
                        || (lo..hi).all(|r| {
                            st.cols[site.col as usize]
                                .rec_init(site.base + (r.wrapping_add(bias) & mask))
                        })
                })
                .then_some(window);
        }
    }
    st.plan = plan;
    if st.timed {
        // rows came, went or were rewritten: which records are in motion, and where, is to be said again
        st.resweep(cx.u, cx.tiles);
    }
    l.edits
        .win
        .store((u64::from(lo) << 32) | u64::from(hi), Ordering::Relaxed);
}

/// Free rows (Integration Card 3, decision D2): store `q` of `parent` holds `n` rows of one record each, row `i`
/// in slot `i`, each with its own rect. Brought up to date by comparing: a row past the old count is written
/// and damaged, a row past the new count is released and damaged, a row whose rect moved is damaged where it
/// was and where it is, and a row whose record differs is stored and damaged (`Row::patch`).
///
/// Identity is positional. A row inserted in the middle therefore re-stores every row after it (each is
/// compared and found to be another record); that is correct and costs the tail, not the world. Keyed rows are
/// the relation's job (`list_sync`).
///
/// One record per row: columns are painted whole, so rows of several records in several columns would not
/// paint in row order. A row of one record is one column, painted in slot order, which is index order.
pub fn free_sync<V, R: Row<V>, F: FnMut(usize) -> (Rect, R), B: Backend>(
    parent: &mut Store,
    q: usize,
    cx: &mut Cx<'_>,
    n: u32,
    mut f: F,
) {
    const { assert!(R::LEAVES == 1, "a free row is one record") };
    let space = parent.space;
    let cap_for = |n: u32| (n.max(1)).next_power_of_two().max(64);
    if q == parent.subs.len() {
        let mut st = Store::new(cap_for(n), space);
        st.structure = parent.structure.clone();
        st.column_count = parent.column_count.clone();
        parent.order.push(Pass::Sub(q as u32));
        parent.subs.push(st);
        parent.bump_structure();
    }
    let st = &mut parent.subs[q];
    let check_site = |st: &Store| {
        assert!(
            st.sites.len() == 1 && st.sites[0].col != u32::MAX,
            "a free row must have one concrete record site"
        );
    };
    if !st.rows.is_empty() {
        check_site(st);
    }
    if st.space != space {
        // the enclosing space moved or was cut otherwise: what the rows showed and what they show
        cx.tiles.mark(st.space.clip);
        cx.tiles.mark(space.clip);
        st.space = space;
    }
    if n > st.cap || st.rows.first().is_some_and(|r| r.0 != tid::<R>()) {
        // more rows than slots, or rows of another record type than the columns were made for (review
        // card3-round1): the columns again; every row is written as new
        list_reset(st, cx, n);
        st.n = 0;
    }
    let mk = Mk {
        org: space.origin(cx.u),
        clip: space.clip,
    };
    let old = st.n;
    for i in 0..n {
        let (rect, row) = f(i as usize);
        if st.rows.is_empty() {
            row_new::<V, R, B>(st, core::slice::from_ref(&rect), &row);
            check_site(st);
        }
        let site = &st.sites[0];
        // SAFETY: the site is a concrete record discovered from `R` (`row_new` and `check_site`); `i` is
        // under the store's capacity (`n <= cap`, checked or made so above); site pointers are current
        // (`discover` ends with `fix_sites`, `list_reset` drops the sites).
        unsafe {
            if i < old {
                let was = site.rect(i);
                if was != rect {
                    cx.tiles.mark(was.shift(mk.org).and(mk.clip));
                    cx.tiles.mark(rect.shift(mk.org).and(mk.clip));
                    site.set_rect(i, rect);
                    cx.changed += 1;
                    cx.moved += 1;
                }
                row.patch(site, i, &mk, cx);
            } else {
                site.set_rect(i, rect);
                row.write(site, i);
                st.row_live[(i >> 6) as usize] |= 1 << (i & 63);
                #[cfg(not(feature = "mutant_free_enter"))]
                cx.tiles.mark(rect.shift(mk.org).and(mk.clip));
                cx.changed += 1;
                cx.moved += 1;
            }
        }
    }
    for i in n..old {
        // SAFETY: slot `i` was live (under the old count), so the site exists and holds it.
        let was = unsafe { st.sites[0].rect(i) };
        #[cfg(not(feature = "mutant_free_leave"))]
        cx.tiles.mark(was.shift(mk.org).and(mk.clip));
        let _ = was;
        st.row_live[(i >> 6) as usize] &= !(1 << (i & 63));
        cx.changed += 1;
        cx.moved += 1;
    }
    (st.n, st.lo, st.hi) = (n, 0, n);
    if st.timed {
        st.resweep(cx.u, cx.tiles);
    }
}

/// A row's rects as its layout gives them, into the lanes of slot `slot`, shifted to where the row is.
struct Lanes {
    sites: *const Site,
    slot: u32,
    at: (i32, i32),
    /// The row's height, and (out) whether some rect left the band `0..row_h` (`Store::spill`).
    row_h: i32,
    spill: bool,
}
impl bough_lay::Out for Lanes {
    #[inline(always)]
    fn put(&mut self, k: usize, r: [i32; 4]) {
        // SAFETY: `k` is under the row's leaves (the layout and the row count alike, checked when the list's
        // columns were made), so `sites + k` is a site of the row. A hole (a scrolled box in the row) has no
        // page: it is skipped (soundness review S13); any other site is a record's and holds `slot`.
        unsafe {
            if (*self.sites.add(k)).col == u32::MAX {
                return;
            }
            self.spill |= (r[1] < 0) | (r[3] > self.row_h);
            (*self.sites.add(k)).set_rect(
                self.slot,
                Rect {
                    x0: r[0] + self.at.0,
                    y0: r[1] + self.at.1,
                    x1: r[2] + self.at.0,
                    y1: r[3] + self.at.1,
                },
            )
        }
    }
}

/// A list of elements. As [`list_sync`], with each row's rects from the row's own layout: a row that enters is
/// laid out into its lanes. Rows are of one height (the first row's); a list whose rows differ in height
/// needs an extent, which the layout lane's `Extent` will give.
#[inline]
pub fn rows_sync<V, R: Row<V> + bough_lay::Shape, F: FnMut(usize) -> R, B: Backend>(
    parent: &mut Store,
    q: usize,
    cx: &mut Cx<'_>,
    n: u32,
    geo: Option<Geo>,
    (edits, how): (&Journal, RowsHow),
    mut f: F,
    at: &'static core::panic::Location<'static>,
) -> i32 {
    const {
        assert!(
            <R as Row<V>>::LEAVES as usize == <R as bough_lay::Shape>::LEAVES,
            "a row's layout has one rect per record"
        )
    };
    let space = parent.space;
    cx.lists |= space.reads();
    if q == parent.subs.len() {
        // the first pass must say where the list is
        let g = geo.expect("a list of elements is first described by a pass that lays out");
        rows_new::<V, R, F, B>(parent, cx, n, g, &mut f, at);
    }
    let st = &mut parent.subs[q];
    if st.src.is_none() && n > 0 {
        // a list that was empty when first described: its columns are made now
        let g = geo.unwrap_or(Geo {
            x: st.lay.0,
            y: st.lay.1,
            w: st.lay.2,
        });
        rows_shape(st, cx, n, g, &mut f);
        let shape = st.row0.clone();
        row_new::<V, R, B>(st, &shape, &f(0));
        st.src = Some((at, <R as Row<V>>::LEAVES, size_of::<R>() as u32));
        st.tmpl = st.row0.clone();
        st.tmpl_at = st.row0.as_ptr() as usize;
    }
    if let Some(g) = geo {
        if (g.x, g.y, g.w) != st.lay {
            // somewhere else, or another width: every exposed row is laid out again
            rows_shape(st, cx, n, g, &mut f);
        }
    }
    let row_h = st.geo.2;
    // a row laid out at no height is content with nothing to show (not refused, as an author's height is)
    admit(n, row_h.max(1));
    if n == 0 || row_h == 0 {
        if edits.follow(&mut st.journal) {
            st.bump_structure();
        }
        edits.win.store(0, Ordering::Relaxed);
        st.moved_from = st.seen;
        st.moved.clear();
        st.moved_ok = false;
        st.seen = edits.ver;
        st.n = n;
        if st.hi > st.lo {
            cx.tiles.mark(space.clip);
            st.row_live.fill(0);
            (st.lo, st.hi) = (0, 0);
            cx.changed += 1;
            cx.moved += 1;
        }
        return 0;
    }
    let (gx, gy, gw) = st.lay;
    let org = space.origin(cx.u);
    let row0 = core::mem::take(&mut st.row0);
    let l = List {
        n,
        row_h,
        x: gx,
        y: gy,
        shape: &row0,
        edits,
    };
    let all = how.all;
    let (lo, hi) = exposed(&l, org, space.clip);
    if (lo == st.lo) & (hi == st.hi) & (st.seen == edits.ver) & !all && edits.follows(&st.journal) {
        st.row0 = row0;
        return n as i32 * row_h;
    }
    let src = (at, <R as Row<V>>::LEAVES, size_of::<R>() as u32);
    if hi - lo > st.cap {
        // the window outgrew the ring: its columns again, larger
        list_reset(st, cx, hi - lo);
        st.lay = (gx, gy, gw);
        st.geo = (gx, gy, row_h);
        row_new::<V, R, B>(st, &row0, &f(lo as usize));
        st.src = Some(src);
        st.tmpl = row0.clone();
    }
    let mk = list_plan(
        st,
        cx,
        &l,
        (lo, hi),
        org,
        space,
        <R as Row<V>>::LEAVES as usize,
        src,
        how,
    );
    st.row0 = row0;
    let (sites, mask, bias) = (st.sites.as_ptr(), st.cap - 1, st.bias);
    let plan = core::mem::take(&mut st.plan);
    for &e in &plan {
        let r = e & 0x3fff_ffff;
        let row = f(r as usize);
        let slot = r.wrapping_add(bias) & mask;
        // SAFETY: the sites were discovered from a row of type `R` (`list_plan` checked its source and shape);
        // `slot` is a slot of the list (`Store::slot_of`).
        unsafe {
            if e >> 31 != 0 {
                if e >> 30 & 1 != 0 {
                    // a row that entered: its own layout, into its lanes
                    // through the list's memo: a row of the shape the memo holds, at a width its answer holds
                    // over, is a copy of the kept rects (bough-lay's `Memo`); otherwise the one layout function
                    let mut out = Lanes {
                        sites,
                        slot,
                        at: (gx, gy + r as i32 * row_h),
                        row_h,
                        spill: false,
                    };
                    st.memo
                        .row(&row, cx.scale, cx.u, gw as f32, (0, 0), &mut out);
                    st.spill |= out.spill;
                }
                row.write(sites, slot);
            } else {
                row.patch(sites, slot, &mk, cx);
            }
        }
    }
    st.plan = plan;
    if st.timed {
        st.resweep(cx.u, cx.tiles);
    }
    edits
        .win
        .store((u64::from(lo) << 32) | u64::from(hi), Ordering::Relaxed);
    n as i32 * row_h
}

/// The first description of a list of elements: lay its first row out, make its columns from that.
#[cold]
#[inline(never)]
fn rows_new<V, R: Row<V> + bough_lay::Shape, F: FnMut(usize) -> R, B: Backend>(
    parent: &mut Store,
    cx: &mut Cx<'_>,
    n: u32,
    g: Geo,
    f: &mut F,
    at: &'static core::panic::Location<'static>,
) {
    let space = parent.space;
    let mut rects = vec![[0i32; 4]; <R as bough_lay::Shape>::LEAVES];
    let mut row_h = 0;
    let first = if n > 0 { Some(f(0)) } else { None };
    if let Some(row) = &first {
        row_h = bough_lay::shape::place(row, cx.scale, cx.u, g.w as f32, &mut rects[..]).0;
    }
    let rows = if row_h > 0 {
        (space.clip.y1 - space.clip.y0) / row_h + 2
    } else {
        64
    };
    let mut st = Store::new((rows as u32).next_power_of_two().max(64), space);
    st.structure = parent.structure.clone();
    st.column_count = parent.column_count.clone();
    st.row0 = rects
        .iter()
        .map(|r| Rect {
            x0: r[0],
            y0: r[1],
            x1: r[2],
            y1: r[3],
        })
        .collect();
    st.lay = (g.x, g.y, g.w);
    st.geo = (g.x, g.y, row_h);
    if let Some(row) = &first {
        let shape = st.row0.clone();
        row_new::<V, R, B>(&mut st, &shape, row);
        st.src = Some((at, <R as Row<V>>::LEAVES, size_of::<R>() as u32));
    }
    // `list_plan` takes the layout as new the first time: nothing is exposed yet
    st.tmpl = st.row0.clone();
    st.tmpl_at = st.row0.as_ptr() as usize;
    parent.order.push(Pass::Sub(parent.subs.len() as u32));
    parent.subs.push(st);
    parent.bump_structure();
}

/// A list of elements is somewhere else or of another width: its first row's layout again, and every exposed
/// row with it.
#[cold]
#[inline(never)]
fn rows_shape<V, R: Row<V> + bough_lay::Shape, F: FnMut(usize) -> R>(
    st: &mut Store,
    cx: &mut Cx<'_>,
    n: u32,
    g: Geo,
    f: &mut F,
) {
    let mut rects = vec![[0i32; 4]; <R as bough_lay::Shape>::LEAVES];
    let row_h = if n > 0 {
        bough_lay::shape::place(&f(0), cx.scale, cx.u, g.w as f32, &mut rects[..]).0
    } else {
        0
    };
    st.row0 = rects
        .iter()
        .map(|r| Rect {
            x0: r[0],
            y0: r[1],
            x1: r[2],
            y1: r[3],
        })
        .collect();
    st.lay = (g.x, g.y, g.w);
    // force `list_plan` to see another layout: the window is emptied and every exposed row comes in again
    st.tmpl.clear();
    st.geo = (g.x, g.y, row_h);
    if st.hi > st.lo {
        cx.tiles.mark(st.space.clip);
    }
    st.row_live.fill(0);
    (st.lo, st.hi) = (0, 0);
}

/// Do the leaves of a row match the columns of a list, type by type?
struct Same<'a> {
    st: &'a Store,
    k: usize,
    ok: bool,
}
impl<V> Leaf<V> for Same<'_> {
    #[inline(always)]
    fn leaf<A: Arch>(&mut self, _: &A, _: TypeId, hov: Option<Hov>) {
        let fits = self.st.sites.get(self.k).is_some_and(|s| {
            let Some(c) = self.st.cols.get(s.col as usize) else {
                return false;
            };

            // The record type, and with it the record's layout. A handler is the same handler if its captures
            // have the layout the column's lane was made for (the same closure of a generic `view` is another
            // type in each sink's instantiation; the element layer's views are not generic and need no such
            // allowance).
            c.arch == TypeId::of::<A>()
                && match (c.hover, hov) {
                    (None, None) => true,
                    (Some(a), Some(b)) => {
                        a.kinds == b.kinds && a.caps == b.caps && a.caps_align == b.caps_align
                    }
                    _ => false,
                }
        });
        self.ok &= fits;
        self.k += 1;
    }
    fn hole(&mut self) {
        self.ok &= self.st.sites.get(self.k).is_some_and(|s| s.col == u32::MAX);
        self.k += 1;
    }
}

/// A list whose rows are of other record types than its columns, or whose window outgrew them: its columns
/// are dropped and made again by the rows now described. Cold.
#[cold]
#[inline(never)]
fn list_reset(st: &mut Store, cx: &mut Cx<'_>, window: u32) {
    cx.tiles.mark(st.space.clip);
    let cap = st.cap.max((window + 2).next_power_of_two()).max(64);
    let (space, seen, stamp) = (st.space, st.seen, st.stamp);
    let structure = st.structure.clone();
    let column_count = st.column_count.clone();
    let journal = st.journal.clone();
    *st = Store::new(cap, space);
    st.structure = structure;
    st.column_count = column_count;
    st.journal = journal;
    st.seen = seen;
    // slots are stamped on from where the old columns stopped: a place held from before is never taken for
    // a row of the new ones
    st.stamp = stamp;
    st.bump_structure();
}

#[cold]
#[inline(never)]
fn list_new(parent: &mut Store, l: &List<'_>) {
    let space = parent.space;
    let rows = if l.n == 0 {
        0
    } else {
        (space.clip.y1 - space.clip.y0) / l.row_h + 2
    };
    let mut st = Store::new((rows as u32).next_power_of_two().max(64), space);
    st.structure = parent.structure.clone();
    st.column_count = parent.column_count.clone();
    st.journal = Some(l.edits.watch());
    st.seen = l.edits.ver;
    parent.order.push(Pass::Sub(parent.subs.len() as u32));
    parent.subs.push(st);
    parent.bump_structure();
}

#[inline(always)]
fn same_template(kept: &[Rect], current: &[Rect]) -> bool {
    fn lanes(rects: &[Rect]) -> &[[i32; 4]] {
        const {
            assert!(size_of::<Rect>() == size_of::<[i32; 4]>());
            assert!(align_of::<Rect>() == align_of::<i32>());
        }
        // SAFETY: Rect is repr(C) and consists of four initialized i32s with no
        // padding. The source slice bounds and shared borrow cover the new slice.
        unsafe { core::slice::from_raw_parts(rects.as_ptr().cast(), rects.len()) }
    }
    lanes(kept).as_flattened() == lanes(current).as_flattened()
}

#[cfg(test)]
#[test]
fn template_comparison_checks_a_later_changed_lane() {
    let rect = Rect::new(-4, 2, 12, 78);
    let original = [rect; 22];
    let mut changed = original;
    changed[21].y1 += 1;
    assert!(same_template(&original, &original));
    assert!(!same_template(&original, &changed));
    assert!(!same_template(&original, &original[..21]));
}

/// Decide what a list pass does, and do everything that does not need the row's type: replay the relation's
/// journal, move the window, lay out rows that entered (their rects are the shape's, shifted), damage rows that
/// entered, left or are other rows than before, and leave in `st.plan` the rows whose operands the caller must
/// write (top bit) or compare.
#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn list_plan(
    st: &mut Store,
    cx: &mut Cx<'_>,
    l: &List<'_>,
    (lo, hi): (u32, u32),
    org: (i32, i32),
    space: Space,
    leaves: usize,
    src: crate::store::RowSrc,
    how: RowsHow,
) -> Mk {
    let all = how.all;
    let rebound = l.edits.follow(&mut st.journal);
    if rebound {
        st.bump_structure();
    }
    let (from, upd, nupd) = if rebound {
        // Version numbers certify edits only within the same journal. A replacement
        // can have the same version while every row holds another item.
        (0, [0; 8], 0)
    } else {
        l.edits.since(st.seen)
    };
    st.moved_ok = if rebound {
        st.moved.clear();
        false
    } else if l.edits.ver.wrapping_sub(st.seen) == nupd as u32 {
        // `since` found every outstanding entry, and each one is an update.
        st.moved.clear();
        true
    } else {
        l.edits.moves_since(st.seen, &mut st.moved)
    };
    st.moved_from = st.seen;
    #[cfg(feature = "mutant_journal")]
    let (from, nupd) = (u32::MAX, 0 * nupd);
    st.seen = l.edits.ver;
    st.set_space(space);
    st.n = l.n;
    st.plan.clear();
    let mk = Mk {
        org,
        clip: space.clip,
    };
    assert!(hi - lo <= st.cap, "the window outgrew the list's capacity");
    assert!(
        l.shape.len() == leaves,
        "a row's layout has one rect per record"
    );
    // The one check per batch: this list's rows are still the rows its columns were made for (same call site,
    // same shape).
    match st.src {
        Some(s) => assert!(
            s.1 == src.1 && s.2 == src.2 && (core::ptr::eq(s.0, src.0) || *s.0 == *src.0),
            "a list's rows changed shape"
        ),
        None => assert!(lo == hi),
    }
    let geo = (l.x, l.y, l.row_h);
    let (mut olo, mut ohi) = (st.lo, st.hi);
    let row_rect = |r: u32| {
        Rect {
            x0: space.clip.x0,
            y0: org.1 + l.y + r as i32 * l.row_h,
            x1: space.clip.x1,
            y1: org.1 + l.y + (r as i32 + 1) * l.row_h,
        }
        .and(space.clip)
    };
    // The owner may mutate the template in place. Compare whole rects even
    // if the template still lives at the address used on the last pass.
    st.tmpl_at = l.shape.as_ptr() as usize;
    if st.geo != geo || !same_template(&st.tmpl, l.shape) {
        // the layout of the rows changed: every exposed row is laid out again
        st.geo = geo;
        st.tmpl.clear();
        st.tmpl.extend_from_slice(l.shape);
        st.spill |= l.shape.iter().any(|t| (t.y0 < 0) | (t.y1 > l.row_h));
        if ohi > olo {
            cx.tiles.mark(space.clip);
        }
        cx.changed += 1;
        cx.moved += 1;
        (olo, ohi) = (0, 0);
    }
    // Inserts and removes as rigid moves. A row that stays exposed across an insert or a remove elsewhere is
    // the same records at another index (and another y): its records are not described again. The window is
    // replayed through the edits in the order they happened: an edit before the window moves every index
    // (the bias moves the other way, so every row keeps its slot); an edit inside it moves the indices on
    // one side, and the rows of the other side, the fewer, are the ones written again. `dirty` is that side,
    // in the indices as they are now. Anything this does not follow exactly (a second edit inside the
    // window, an edit after one) writes every exposed row again, which is always right.
    let mut from = from;
    let mut dirty = (0u32, 0u32);
    let mut rigid = false;
    if (how.by_item | l.edits.by_item)
        && from != u32::MAX
        && st.moved_ok
        && ohi > olo
        && !st.moved.is_empty()
    {
        rigid = true;
        let mut lost = false;
        for i in 0..st.moved.len() {
            if dirty.1 > dirty.0 {
                lost = true;
                break;
            }
            match st.moved[i] {
                Edit::Insert(at) if at <= olo => {
                    st.bias = st.bias.wrapping_sub(1);
                    (olo, ohi) = (olo + 1, ohi + 1);
                    shift_rows(st, leaves, (olo, ohi), l.row_h);
                }
                Edit::Insert(at) if at < ohi => {
                    if at - olo <= ohi - at {
                        // the rows above are the fewer: the rows from `at` on keep their slots
                        st.bias = st.bias.wrapping_sub(1);
                        shift_rows(st, leaves, (at + 1, ohi + 1), l.row_h);
                        dirty = (olo, at + 1);
                    } else {
                        dirty = (at, ohi + 1);
                    }
                    ohi += 1;
                }
                Edit::Remove(at) if at < olo => {
                    st.bias = st.bias.wrapping_add(1);
                    (olo, ohi) = (olo - 1, ohi - 1);
                    shift_rows(st, leaves, (olo, ohi), -l.row_h);
                }
                Edit::Remove(at) if at < ohi => {
                    if at - olo <= ohi - at - 1 {
                        st.bias = st.bias.wrapping_add(1);
                        shift_rows(st, leaves, (at, ohi - 1), -l.row_h);
                        dirty = (olo, at);
                    } else {
                        dirty = (at, ohi - 1);
                    }
                    ohi -= 1;
                }
                // past the window: nothing exposed moved
                Edit::Insert(_) | Edit::Remove(_) => {}
                Edit::Update(_) | Edit::Shift(_) => {
                    unreachable!("`moves_since` keeps inserts and removes")
                }
            }
        }
        if lost {
            (olo, ohi) = (0, 0);
            dirty = (0, 0);
        }
        // everything from the first edit on is somewhere else on screen
        cx.tiles.mark(space.clip);
        from = u32::MAX;
    }
    // A changed window or rigid edit moves what a reader sees. Ordinary updates
    // count only where the record patch stores a value.
    if (lo != olo) | (hi != ohi) | (from != u32::MAX) | rigid {
        cx.changed += 1;
        cx.moved += 1;
    }
    if (lo == olo) & (hi == ohi) & (from == u32::MAX) & !rigid {
        if all {
            // Every currently exposed record is compared. With the same window and no
            // structural edit, no damage or new exposure can arise from planning these rows.
            st.plan.extend(lo..hi);
        } else {
            for &r in &upd[..nupd] {
                if (r >= lo) & (r < hi) {
                    st.plan.push(r);
                }
            }
        }
        return mk;
    }
    // Only the intervals outside the new window lost pixels.
    for r in (olo..ohi.min(lo)).chain(olo.max(hi)..ohi) {
        cx.tiles.mark(row_rect(r));
    }
    for r in lo..hi {
        let fresh = (r < olo) | (r >= ohi) | ((r >= dirty.0) & (r < dirty.1));
        if fresh | (r >= from) {
            // A row that is new here, or another row than was here: its records are written without being
            // compared, and the row is damaged as a whole.
            if fresh {
                let at = (l.x, l.y + r as i32 * l.row_h);
                let sites = &st.sites[..leaves];
                let slot = st.slot_of(r);
                if st.has_holes {
                    for (s, t) in sites.iter().zip(l.shape) {
                        if s.col != u32::MAX {
                            // SAFETY: a non-hole site of this list, at its physical slot.
                            unsafe { s.set_rect(slot, t.shift(at)) }
                        }
                    }
                } else {
                    for (s, t) in sites.iter().zip(l.shape) {
                        // SAFETY: this list has no holes and the slot is under its capacity.
                        unsafe { s.set_rect(slot, t.shift(at)) }
                    }
                }
            }
            cx.tiles.mark(row_rect(r));
            st.plan.push(r | 1 << 31 | (fresh as u32) << 30);
            // another row is in this slot from now on
            if st.row_stamp.len() != st.cap as usize {
                st.row_stamp.resize(st.cap as usize, 0);
            }
            st.stamp = st.stamp.checked_add(1).expect("row stamp exhausted");
            let slot = st.slot_of(r) as usize;
            st.row_stamp[slot] = st.stamp;
        } else if all | upd[..nupd].contains(&r) {
            st.plan.push(r);
        }
    }
    st.lo = lo;
    st.hi = hi;
    let slot = st.slot_of(lo) as usize;
    publish_window(&mut st.row_live, slot, (hi - lo) as usize);
    mk
}

/// Rows `rows.0..rows.1` of a list (indices as they are now, slots by the store's bias as it is now) are `dy`
/// further down: every record of each keeps its slot and gets its rect there.
fn shift_rows(st: &mut Store, leaves: usize, rows: (u32, u32), dy: i32) {
    #[cfg(feature = "mutant_rigid")]
    let dy = 0 * dy;
    for r in rows.0..rows.1 {
        let slot = st.slot_of(r);
        for s in &st.sites[..leaves] {
            if s.col == u32::MAX {
                continue;
            }
            // SAFETY: a record's site of this list (not a hole); the slot is under its capacity.
            unsafe {
                let mut b = s.rect(slot);
                b.y0 += dy;
                b.y1 += dy;
                s.set_rect(slot, b);
            }
        }
    }
}

/// A live window has at most two ranges in the ring bitmap. This tracks exposure only.
#[inline]
fn publish_window(words: &mut [u64], start: usize, rows: usize) {
    let cap = words.len() * 64;
    let end = start + rows;
    let wrapped = end.saturating_sub(cap);
    let low_bits = |n: usize| -> u64 { if n >= 64 { u64::MAX } else { (1u64 << n) - 1 } };
    for (index, word) in words.iter_mut().enumerate() {
        let origin = index * 64;
        let top = low_bits(end.saturating_sub(origin));
        let bottom = low_bits(start.saturating_sub(origin));
        *word = (top & !bottom) | low_bits(wrapped.saturating_sub(origin));
    }
}

#[cfg(test)]
mod window_tests {
    use super::publish_window;

    #[test]
    fn window_masks_match_each_exposed_slot() {
        for cap in [64, 128, 256] {
            let mut actual = vec![0; cap / 64];
            let mut expected = actual.clone();
            for start in 0..cap {
                for len in [0, 1, 2, 63, 64, cap - 1, cap] {
                    expected.fill(0);
                    for off in 0..len {
                        let slot = (start + off) & (cap - 1);
                        expected[slot >> 6] |= 1 << (slot & 63);
                    }
                    publish_window(&mut actual, start, len);
                    assert_eq!(actual, expected, "start {start}, len {len}, cap {cap}");
                }
            }
        }
    }
}

#[cfg(test)]
mod journal_watch_tests {
    use super::*;

    #[test]
    fn follow_keeps_existing_watch_and_rebinds_to_new_journal() {
        let mut first = Journal::new();
        let mut second = Journal::new();
        let mut followed = None;
        first.follow(&mut followed);
        let old = followed.as_ref().unwrap().clone();
        first.follow(&mut followed);
        assert!(std::sync::Arc::ptr_eq(followed.as_ref().unwrap(), &old));
        first.push(Edit::Update(0));
        assert_eq!(followed.as_ref().unwrap().load(Ordering::Relaxed), 1);

        second.follow(&mut followed);
        assert!(!std::sync::Arc::ptr_eq(followed.as_ref().unwrap(), &old));
        first.push(Edit::Update(0));
        assert_eq!(followed.as_ref().unwrap().load(Ordering::Relaxed), 0);
        second.push(Edit::Update(0));
        assert_eq!(followed.as_ref().unwrap().load(Ordering::Relaxed), 1);
    }
}
