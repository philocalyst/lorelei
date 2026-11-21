//! The driver: one store tree, the block the raster last saw, the damage.
use crate::event::{
    Button, Dispatch, ENV_REDUCED_MOTION, Ev, Event, Key, Mods, NONE, PLACE_REC_BITS,
    REDUCED_MOTION_AHEAD, kind,
};
use crate::expr::{
    APP_WORDS, HOT, Slot, U, U_ENV, U_FOCUS, U_PICK, U_PRESS, U_PRESS_AT, U_PTR_X, U_PTR_Y,
    U_SCALE, U_TIME,
};
use crate::sink::{Cx, Patch, View, Write};
use crate::store::{Backend, ColEnt, Path, Space, Store, StoreOwner, Tiles};
use core::marker::PhantomData;
use core::sync::atomic::Ordering;

/// Published metadata for a column. Stores come from their owners. A view pass prevents all table
/// reads until it returns, and a change to store structure requires a new table before reading it.
#[derive(Clone, Copy)]
struct Column {
    store: *const Store,
    kinds: u32,
    index: u16,
    fast: u16,
}

/// An unpublished table. Store structure revisions count up from 1 with a checked add.
const STALE: u64 = 0;
// A stateful record can change on a delta, without waiting for a view pass.
const HOVER_STATE: u32 = 1 << 31;

/// Non-reusable identity of one world instance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorldId(u64);
impl WorldId {
    fn fresh() -> Self {
        static NEXT: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(1);
        Self(
            NEXT.try_update(Ordering::Relaxed, Ordering::Relaxed, |v| v.checked_add(1))
                .expect("world identity space exhausted"),
        )
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Place {
    world: WorldId,
    packed: u32,
    /// The stamp of the place's slot when the token was made (`Store::row_stamp`): the token names that
    /// record, not whatever row a list later puts in its slot (review TR1-02).
    stamp: u64,
}
impl Place {
    pub fn packed(self) -> u32 {
        self.packed
    }
}
/// A held button: where it went down, the record it went down on, and the record whose drag handler captured
/// the pointer (`NONE`: none).
#[derive(Clone, Copy, Debug)]
struct Held {
    place: u32,
    /// What the pressed place showed (store, site, row): a release activates it only if it still shows that.
    what: Option<Focused>,
    drag: u32,
    at: (i32, i32),
    button: Button,
}

/// The focused record by what it is (store, site, row), so that focus survives its row leaving exposure and
/// coming back; its place is recomputed whenever a window may have moved.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Focused {
    path: Path,
    site: u32,
    row: u32,
    /// The list's relation version this row index is right for (inserts and removes after it remap it).
    seen: u32,
}

/// Work performed by the most recent delta.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DeltaStats {
    pub visited_incidences: usize,
    pub evaluated_columns: usize,
    pub dense_fallbacks: usize,
}

/// What the bounded deltas (`store::fast`) did since the world was made: the count gates read these.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FastStats {
    /// Deltas answered from a column's key index, and the records they evaluated.
    pub key_hits: u64,
    pub key_records: u64,
    /// Key indexes built (a column took the keyed path twice with no description pass between).
    pub key_builds: u64,
    /// Changes of only a key's word that were answered by the page scan (no index yet).
    pub key_scans: u64,
    /// Pointer deltas answered by reach, the records they evaluated, and page-box sets built.
    pub near_hits: u64,
    pub near_records: u64,
    pub near_builds: u64,
    /// Column deltas answered as "the whole space" because a broad word changed ([`World::broad`]).
    pub broad_hits: u64,
}

/// A column's bounded deltas and what the world keeps for them. The index and the boxes are of the records as
/// they were at description pass `valid`; any later pass (`World::wrote`) makes them nothing.
struct Fast {
    fns: crate::store::fast::FastFns,
    key_valid: u64,
    key_asked: u64,
    keys: Vec<(u32, u32)>,
    box_valid: u64,
    box_asked: u64,
    boxes: Vec<[i32; 4]>,
}

/// A hovered record as it was entered (`World::hov_kept`): its handler face, and where its record and its
/// captures are copied in `hov_bytes` (in 16-byte units). For an over-aligned copy, `rec == u32::MAX`
/// and `caps` identifies the owner in `hov_aligned`.
#[derive(Clone, Copy)]
struct Kept {
    ons: Option<crate::store::Hov>,
    rec: u32,
    caps: u32,
}

/// Owned storage for the rare record or capture whose alignment exceeds the inline arena's.
struct AlignedHover {
    ptr: core::ptr::NonNull<u8>,
    layout: std::alloc::Layout,
    captures: usize,
}
impl Drop for AlignedHover {
    fn drop(&mut self) {
        // SAFETY: this allocation belongs to this owner, with exactly this layout.
        unsafe { std::alloc::dealloc(self.ptr.as_ptr(), self.layout) };
    }
}

/// The records of every column keyed by one block word, by key: `(key, column in the table, record)`, sorted.
/// When every column that reads the word is keyed by it, a change of that word alone concerns the records
/// keyed by its old and its new value, whichever columns they are in: two searches here instead of one visit
/// per reading column (a view with one `hover_bg` column per button has as many columns as buttons). Built
/// and dropped on the per-column index's terms (`Fast`).
struct WordKeys {
    word: u8,
    valid: u64,
    asked: u64,
    /// The table shape this was decided for, and whether every reader of the word is keyed by it.
    shape: u64,
    all_keyed: bool,
    keys: Vec<(u32, u32, u32)>,
}

/// A world accepts only its statically branded view and backend.
/// Wrong-owner state cannot reach a gathered reader or event trampoline.
///
/// ```compile_fail
/// use bough_x::{prelude::*, soft::Soft};
/// struct Wide(U);
/// struct Small(U);
/// impl View for Wide {
///     fn block(&self) -> &U { &self.0 }
///     fn view<S: Sink<Self>>(&self, _: &mut S) {}
/// }
/// impl View for Small {
///     fn block(&self) -> &U { &self.0 }
///     fn view<S: Sink<Self>>(&self, _: &mut S) {}
/// }
/// let mut world = World::<Wide, Soft>::new(32, 32, 1);
/// world.mount(&Small(U::default()));
/// ```
pub struct World<V, B> {
    id: WorldId,
    inputs: crate::el::InputToken,
    /// Private (review C01):
    /// its columns' descriptors were made for `B` and for the view mounted; outside code reads it through
    /// [`World::root`] and changes it only through the world's own passes.
    root: StoreOwner,
    /// The block as of the last settle: what the pixels on screen were shaded with.
    pub shown: U,
    pub tiles: Tiles,
    /// The words the application uses; the compare stops there.
    nwords: usize,
    /// The block words that move a list's exposure. A watched relation changing also drives the view.
    lists: u64,
    /// The block words some column reads: a pointer event that no record reads costs nothing.
    reads: u64,
    /// Every column of the world in paint order: the words each reads (dense, scanned on a block change), its
    /// entry (reached only on a match), and where it came from (cold). Rebuilt when the tree changes shape.
    col_reads: Vec<u64>,
    /// Per block word, the columns that read it (`by_word[w]..by_word[w + 1]` in `readers`): a write to one
    /// word visits its readers and no other column.
    by_word: [u32; 65],
    readers: Vec<u32>,
    union_epoch: u32,
    union_seen: Vec<u32>,
    pub delta_stats: DeltaStats,
    pub fast_stats: FastStats,
    /// The bounded-delta state indexed by a column entry.
    fast: Vec<Fast>,
    /// Per block word that some delta asked about alone: its records by key across columns.
    word_keys: Vec<WordKeys>,
    key_words: u64,
    key_scratch: Vec<(u32, u32)>,
    /// The number of the last description pass (mount, a list pass, a re-description): the only things that
    /// write a record's operands or change which records are live.
    wrote: u64,
    /// Switch the bounded deltas off (every delta is the page scan): what the gates compare them against.
    pub no_fast: bool,
    /// The block words whose change is taken to change everything that reads them (a theme, a density, a
    /// scale): a column that reads one is not differentiated record by record when it changes; its space is
    /// damaged as a whole, one mark, whatever the column holds. What is shown is the same (a superset of the
    /// scan's damage is repaired); the trade is evaluations in the turn for tiles in the frame, the right one
    /// when nearly every reader changes and the frame goes to an executor that shades a full surface cheaply.
    /// Columns with core-written state ([`Arch::STATE`]) keep the scan (their transitions start from what
    /// each record shows). Zero by default: nothing is broad unless the application says so.
    pub broad: u64,
    /// The clip the current delta last marked for a broad word (so a space is marked once a delta).
    broad_last: Option<crate::store::Rect>,
    cols: Vec<Column>,
    spaces: Vec<(u64, *const Store)>,
    shape: u64,
    /// Remains set if a view pass unwinds. A later complete description may publish again.
    pass_open: bool,
    /// Published journal bindings. The watched source belongs to the active table. The store carries
    /// the version consumed by the latest successful list pass.
    relations: Vec<(*const Store, std::sync::Arc<core::sync::atomic::AtomicU32>)>,
    pub retables: usize,
    mounts: u32,
    resizes: u32,
    /// The fields of the root view the block's words are gathered from, when the view keeps its Frame state
    /// as its own fields (learned from the columns' lenses; empty for a view that declares a block).
    pub(crate) slots: Vec<Slot>,
    /// The clock (`tick`'s argument). The raster's `Time` runs ahead of it under reduced motion.
    clock: u32,
    /// The pointer, and the places under it whose handlers hear hover, bottom first.
    ptr: Option<(i32, i32)>,
    hov: Vec<u32>,
    held: Option<Held>,
    focus: Option<Focused>,
    /// The columns whose records have handlers: index in the table and the kinds they hear. The pointer path
    /// visits these and no other column.
    handlers: Vec<(u32, u32)>,
    /// Publication revision: grows whenever a pass stored records with other words or rects, or moved a list.
    revision: u32,
    /// Grows whenever something shown may be somewhere else than it was: a rect, a list's rows, a space
    /// that moved. What a host compares to know whether the pick under a still pointer is to be taken
    /// again ([`World::repick`]).
    motion: u32,
    /// A typed world may dispatch only after mounting its view.
    mounted: bool,
    /// The row each hovered place showed when it was entered (a list's slot shows another row after a scroll).
    hov_what: Vec<u64>,
    /// A copy of each hovered record and its handler's captures as they were when the place was entered, with
    /// the handler face they belong to: what a leave is delivered with when the slot has since been given to
    /// another row (the item that was entered hears the leave, not the row now in its slot).
    hov_kept: Vec<Kept>,
    hov_bytes: Vec<core::mem::MaybeUninit<u128>>,
    hov_aligned: Vec<AlignedHover>,
    /// Scratch for the event path, reserved once: an event allocates nothing.
    buf: Vec<u32>,
    pool: Vec<Vec<u32>>,
    _b: PhantomData<fn(V, B) -> (V, B)>,
}

impl<V: View, B: Backend> World<V, B> {
    pub fn new(w: i32, h: i32, nwords: usize) -> World<V, B> {
        let id = WorldId::fresh();
        let mut root = Store::new(1, Space::root(w, h));
        root.world = Some(id);
        let mut shown = U::default();
        for w in [U_PICK, U_FOCUS, U_PRESS] {
            shown.w[w] = NONE;
        }
        shown.w[U_SCALE] = 1f32.to_bits();
        World {
            id,
            inputs: crate::el::InputToken::new(),
            root: StoreOwner::new(root),
            shown,
            tiles: Tiles::new(w, h),
            nwords,
            lists: 0,
            reads: 0,
            col_reads: Vec::new(),
            by_word: [0; 65],
            readers: Vec::new(),
            union_epoch: 0,
            union_seen: Vec::new(),
            delta_stats: DeltaStats::default(),
            fast_stats: FastStats::default(),
            fast: Vec::new(),
            word_keys: Vec::new(),
            key_words: 0,
            key_scratch: Vec::new(),
            wrote: 1,
            no_fast: false,
            broad: 0,
            broad_last: None,
            cols: Vec::new(),
            spaces: Vec::new(),
            shape: STALE,
            pass_open: false,
            relations: Vec::new(),
            retables: 0,
            mounts: 0,
            resizes: 0,
            clock: 0,
            ptr: None,
            revision: 0,
            motion: 0,
            mounted: false,
            slots: Vec::new(),
            hov_what: Vec::with_capacity(64),
            hov_kept: Vec::with_capacity(64),
            hov_bytes: Vec::with_capacity(64),
            hov_aligned: Vec::new(),
            hov: Vec::with_capacity(64),
            held: None,
            focus: None,
            handlers: Vec::new(),
            buf: Vec::with_capacity(64),
            pool: (0..4).map(|_| Vec::with_capacity(64)).collect(),
            _b: PhantomData,
        }
    }

    #[inline]
    fn relations_changed(&self) -> bool {
        self.shape == STALE
            || self.relations.iter().any(|(store, watch)| {
                // SAFETY: the published table is current, and the shared world borrow excludes a view
                // pass. Each pointer is an owning store address recorded when the table was built.
                watch.load(Ordering::Relaxed) != unsafe { (**store).seen }
            })
    }

    pub fn input_revision(&self) -> u64 {
        self.inputs.revision()
    }
    pub fn id(&self) -> WorldId {
        self.id
    }
    pub fn place_token(&mut self, path: &Path, col: u32, rec: u32) -> Option<Place> {
        let packed = self.place(path, col, rec);
        self.at_place(packed)?;
        Some(Place {
            world: self.id,
            packed,
            stamp: self.stamp_at(packed),
        })
    }
    pub fn at_token(&self, place: Place) -> Option<(&Store, u32, u32)> {
        if place.world != self.id || self.stamp_at(place.packed) != place.stamp {
            return None;
        }
        self.at_place(place.packed)
    }
    /// The application's block with the words it does not own (the surface's and the raster's) as the world
    /// has them.
    #[inline(always)]
    fn block(&self, v: &V) -> U {
        let mut scratch = U::default();
        let mut u = *v.gathered(&self.slots, &mut scratch);
        u.w[APP_WORDS..].copy_from_slice(&self.shown.w[APP_WORDS..]);
        u
    }
    /// The words of the application's block that differ from what is shown.
    #[inline]
    pub fn changed_words(&self, v: &V) -> u64 {
        let mut scratch = U::default();
        self.shown
            .diff(v.gathered(&self.slots, &mut scratch), self.nwords)
            & !HOT
    }
    /// The store tree, to read (statistics, reflection, accessibility).
    #[inline(always)]
    pub fn root(&self) -> &Store {
        &self.root
    }
    /// The damage the block going from `old` to `new` (`ch`: the changed words) gives, by a walk of the store
    /// tree, into `into` (or the world's own damage): the reference the world's delta (through its column
    /// table) is compared with. Records in transition take over from `old` as in a turn.
    pub fn tree_delta(&mut self, old: &U, new: &U, ch: u64, into: Option<&mut Tiles>) {
        if self.pass_open {
            return;
        }
        match into {
            Some(t) => self.root.delta(old, new, ch, t),
            None => self.root.delta(old, new, ch, &mut self.tiles),
        }
    }
    /// The window is `w` x `h` device pixels now. The world is kept: stores, focus, a held press and hover stay
    /// what they were (no remount); the damage plane is made for the new size and marked whole, and the caller
    /// re-describes (`redescribe`), which stores the records whose rects the new size moved.
    pub fn resize(&mut self, w: i32, h: i32) {
        let sweeps = core::mem::take(&mut self.tiles.sweeps);
        self.tiles = Tiles::new(w, h);
        self.tiles.sweeps = sweeps;
        self.tiles.mark_all();
        let was = self.root.space;
        self.root.space = Space {
            clip: crate::store::Rect {
                x0: 0,
                y0: 0,
                x1: w,
                y1: h,
            },
            ..was
        };
        self.resizes += 1;
    }
    /// Store one free row again (`Sink::free`): row `slot` of the free rows under scope `scope` of the root.
    /// Compared with what the slot holds; stored and damaged if it differs. This is a change of one record
    /// without a description (a hover paint). Returns whether the record differed. Refused (a panic) if there
    /// is no such row or its record is of another type.
    pub fn free_set<R: crate::sink::Row<V>>(&mut self, scope: usize, slot: u32, row: R) -> bool {
        const { assert!(R::LEAVES == 1, "a free row is one record") };
        self.capture_hover();
        let st = self
            .root
            .subs
            .get_mut(scope)
            .and_then(|s| s.subs.get_mut(0))
            .expect("no such scope of free rows");
        assert!(
            st.rows.first().map(|r| r.0) == Some(crate::sink::tid::<R>()),
            "free rows of another record type"
        );
        assert!(
            st.sites.len() == 1 && st.src.is_none(),
            "not a store of free rows"
        );
        assert!(
            slot < st.n && st.row_live[(slot >> 6) as usize] >> (slot & 63) & 1 != 0,
            "no such free row"
        );
        let mk = crate::sink::Mk {
            org: st.space.origin(&self.shown),
            clip: st.space.clip,
        };
        let mut cx = Cx {
            tiles: &mut self.tiles,
            u: &self.shown,
            all: false,
            lists: 0,
            regroup: false,
            changed: 0,
            moved: 0,
            scale: f32::from_bits(self.shown.w[U_SCALE]),
        };
        // SAFETY: the store's one site was discovered from a row of type `R` (checked just above against the
        // type its rows were made for), `slot` is a live row of it, and site pointers are current (they are
        // fixed whenever a column grows).
        unsafe { row.patch(&st.sites[0], slot, &mk, &mut cx) };
        let changed = cx.changed;
        // a record was stored outside a description pass: what was cached against the last pass is stale
        self.wrote += (changed != 0) as u64;
        self.revision = self.revision.wrapping_add(changed);
        changed != 0
    }
    /// How many times the world was resized in place, and how many times it mounted (count bands for the gates:
    /// a resize is not a mount).
    pub fn lifecycle(&self) -> (u32, u32) {
        (self.mounts, self.resizes)
    }
    /// Device pixels per point. Set before the first mount; a change afterwards is a re-description.
    pub fn set_scale(&mut self, scale: f32) {
        self.shown.w[U_SCALE] = scale.to_bits();
    }
    /// The publication revision: it moves whenever a pass stored a record with other words or another rect, or
    /// moved a list. A reader of the stores that saw the same revision and the same block has nothing new to
    /// read.
    #[inline(always)]
    pub fn revision(&self) -> u32 {
        self.revision
    }
    /// See the field: moves when a pass or a delta may have put other content under a still pointer. (A
    /// record that moves by its own paint with a block word, `Arch::MOVES`, is not counted.)
    #[inline(always)]
    pub fn motion(&self) -> u32 {
        self.motion
    }
    /// The time of the frame being prepared, by the clock: what a handler stamps a motion's start with.
    #[inline(always)]
    pub fn now(&self) -> u32 {
        self.clock
    }

    /// Describe the view for the first time. Only what is exposed comes to exist.
    ///
    /// A view whose Frame state is its own fields (the element layer) is read through the table of slots its
    /// columns name, which exists only after a first description: that description saw those fields as zero.
    /// It is then described again, from nothing, with them read (cold, once per mount; a view that declares its
    /// block is described once). Again rather than settled from zero: a transition must not run from a value
    /// that was never shown.
    pub fn mount(&mut self, v: &V) {
        self.mounts += 1;
        for _ in 0..8 {
            self.mount_pass(v);
            let mut scratch = U::default();
            if self
                .shown
                .diff(v.gathered(&self.slots, &mut scratch), self.nwords)
                & !HOT
                == 0
            {
                return;
            }
            self.shape = STALE;
            self.cols.clear();
            *self.root = Store::new(1, self.root.space);
            self.tiles.sweeps.clear();
        }
        panic!("the view's fields named by its columns did not settle in 8 descriptions");
    }
    fn mount_pass(&mut self, v: &V) {
        self.capture_hover();
        self.wrote += 1;
        self.root.world = Some(self.id);
        self.mounted = true;
        self.shown = self.block(v);
        self.tiles.mark_all();
        self.root.row_live[0] = 1;
        let mut cx = Cx {
            tiles: &mut self.tiles,
            u: &self.shown,
            all: true,
            lists: 0,
            regroup: false,
            changed: 0,
            moved: 0,
            scale: f32::from_bits(self.shown.w[U_SCALE]),
        };
        // A pass publishes a new table only after the view returns. An unwound pass leaves it unavailable.
        self.shape = STALE;
        self.cols.clear();
        self.pass_open = true;
        crate::el::track_inputs(&self.inputs, || {
            v.view(&mut Write::<V, B>::new(&mut self.root, &mut cx, true))
        });
        self.pass_open = false;
        self.lists = cx.lists;
        self.revision = self.revision.wrapping_add(cx.changed + 1);
        self.motion = self.motion.wrapping_add(cx.moved + 1);
        self.root.resweep(&self.shown, &mut self.tiles);
        self.retable();
        self.refocus();
    }

    /// After a pass that made columns: the slots they name are learned, and a word of the view's own fields
    /// that no column named before is gathered now and settled like any other change. Bounded: each round
    /// follows a pass that changed the tree's shape, and a view has finitely many fields to name.
    #[inline(never)]
    fn learn(&mut self, v: &V) {
        for _ in 0..8 {
            if self.shape != STALE {
                return;
            }
            self.retable();
            let mut scratch = U::default();
            let ch = self
                .shown
                .diff(v.gathered(&self.slots, &mut scratch), self.nwords)
                & !HOT;
            if ch == 0 {
                return;
            }
            self.changed(v, ch, false);
        }
        // eight rounds and the shape still moved: the table must not outlive the tree it was built from
        if self.shape == STALE {
            self.retable();
        }
    }

    /// Bring the column table up to date with the store tree. Cold: the tree changes shape when a column or a
    /// child store is first described.
    #[cold]
    #[inline(never)]
    fn retable(&mut self) {
        self.retables += 1;
        self.shape = STALE;
        let mut cols = core::mem::take(&mut self.cols);
        cols.clear();
        self.spaces.clear();
        self.relations.clear();
        fn visit(
            owner: &StoreOwner,
            cols: &mut Vec<Column>,
            spaces: &mut Vec<(u64, *const Store)>,
            relations: &mut Vec<(*const Store, std::sync::Arc<core::sync::atomic::AtomicU32>)>,
        ) {
            let store = owner.as_ptr();
            if let Some(watch) = &owner.journal {
                relations.push((store, watch.clone()));
            }
            let reads = owner.space.reads();
            if reads != 0 {
                spaces.push((reads, store));
            }
            for pass in &owner.order {
                match *pass {
                    crate::store::Pass::Col(index) => cols.push(Column {
                        store,
                        index: u16::try_from(index).expect("too many columns in a store"),
                        fast: u16::MAX,
                        kinds: 0,
                    }),
                    crate::store::Pass::Sub(child) => {
                        visit(owner.subs.owner(child as usize), cols, spaces, relations)
                    }
                }
            }
        }
        visit(&self.root, &mut cols, &mut self.spaces, &mut self.relations);
        self.handlers.clear();
        self.col_reads.clear();
        self.fast.clear();
        for index in &mut self.word_keys {
            index.shape = STALE;
        }
        for (k, route) in cols.iter_mut().enumerate() {
            // SAFETY: the table is current, and no view pass can run while it is borrowed here.
            let st = unsafe { &*route.store };
            let c = &st.cols[route.index as usize];
            if let Some(o) = c.hover {
                self.handlers.push((k as u32, o.kinds));
                route.kinds = o.kinds | (u32::from(c.desc.state) * HOVER_STATE);
            }
            self.col_reads.push(c.reads);
            let fns = c.desc.fast;
            if fns.key.is_some() || fns.near.is_some() {
                #[cfg(not(feature = "mutant_fastcol"))]
                {
                    route.fast = u16::try_from(self.fast.len()).expect("too many fast columns");
                }
                self.fast.push(Fast {
                    fns,
                    key_valid: 0,
                    key_asked: 0,
                    keys: Vec::new(),
                    box_valid: 0,
                    box_asked: 0,
                    boxes: Vec::new(),
                });
            }
        }
        assert!(
            cols.len() <= 1 << (32 - PLACE_REC_BITS),
            "a place names one of at most 4,096 columns"
        );
        self.union_seen.clear();
        self.union_seen.resize(cols.len(), 0);
        self.readers.clear();
        self.key_words = 0;
        for w in 0..64 {
            self.by_word[w] = self.readers.len() as u32;
            self.readers.extend(
                (0..cols.len())
                    .filter(|&k| self.col_reads[k] >> w & 1 != 0)
                    .map(|k| k as u32),
            );
            let readers = &self.readers[self.by_word[w] as usize..];
            if readers.len() >= 2 && readers.iter().all(|&k| {
                let fast = cols[k as usize].fast;
                fast != u16::MAX && matches!(self.fast[fast as usize].fns.key, Some((word, ..)) if word as usize == w)
            }) {
                self.key_words |= 1 << w;
            }
        }
        self.by_word[64] = self.readers.len() as u32;
        // The root view's fields the columns' lenses name. Two fields in one word cannot both be Frame state.
        self.slots.clear();
        for route in &cols {
            // SAFETY: the table is current, and no view pass can run while it is borrowed here.
            let st = unsafe { &*route.store };
            let d = st.cols[route.index as usize].desc;
            (d.slots)(
                &mut |s: Slot| match self.slots.iter().find(|t| t.index() == s.index()) {
                    Some(t) => assert!(
                        *t == s,
                        "two fields of the view share word {} of the block (byte offsets {} and {}): Frame state must lie within {} bytes",
                        s.index(),
                        t.offset(),
                        s.offset(),
                        APP_WORDS * 4
                    ),
                    None => self.slots.push(s),
                },
            );
        }
        // what a column reads, and what moves a space
        self.reads = self
            .spaces
            .iter()
            .fold(self.root.reads_all(), |m, s| m | s.0);
        self.cols = cols;
        self.shape = self.root.structure_revision();
    }

    /// The damage of the block going from what is shown to `u` (`ch`: the changed words). The path from here
    /// to a page is this world, the column's entry, the page: the store tree is not on it.
    #[inline(never)]
    fn delta(&mut self, u: &U, ch: u64) {
        self.delta_stats = DeltaStats::default();
        self.broad_last = None;
        if !self.columns() {
            return;
        }
        let relevant = ch & self.reads;
        if relevant == 0 {
            return;
        }
        for &(reads, key) in &self.spaces {
            if reads & ch != 0 {
                // SAFETY: `delta` refreshed a stale table before accessing its stores.
                let st = unsafe { &*key };
                // the space moved: everything it shows is somewhere else
                self.tiles.mark(st.space.clip);
                self.motion = self.motion.wrapping_add(1);
                if st.timed & (st.reads & ch != 0) {
                    st.resweep(u, &mut self.tiles);
                }
            }
        }
        if relevant & relevant.wrapping_sub(1) == 0 {
            // A single reader list contains each column at most once.
            let word = relevant.trailing_zeros() as usize;
            let start = self.by_word[word] as usize;
            let end = self.by_word[word + 1] as usize;
            if self.key_words & relevant != 0 && self.delta_word_keys(word, start, end, u) {
                return;
            }
            self.delta_stats.visited_incidences = end - start;
            for i in start..end {
                self.delta_one(self.readers[i] as usize, u, ch);
            }
            return;
        }

        let mut words = relevant;
        let mut incidences = 0usize;
        while words != 0 {
            let w = words.trailing_zeros() as usize;
            incidences += (self.by_word[w + 1] - self.by_word[w]) as usize;
            words &= words - 1;
        }
        // Compare the exact number of edges with the number of dense entries.
        if incidences > self.col_reads.len() {
            self.delta_stats.dense_fallbacks = 1;
            for k in 0..self.col_reads.len() {
                if self.col_reads[k] & ch != 0 {
                    self.delta_one(k, u, ch);
                }
            }
        } else {
            self.union_epoch = self.union_epoch.wrapping_add(1);
            if self.union_epoch == 0 {
                self.union_seen.fill(0);
                self.union_epoch = 1;
            }
            let mut words = relevant;
            #[cfg(feature = "mutant_sparse_union")]
            if words.count_ones() > 1 {
                let first = 1u64 << words.trailing_zeros();
                let rest = words & !first;
                words &= !(1u64 << rest.trailing_zeros());
            }
            while words != 0 {
                let w = words.trailing_zeros() as usize;
                for i in self.by_word[w] as usize..self.by_word[w + 1] as usize {
                    self.delta_stats.visited_incidences += 1;
                    let k = self.readers[i] as usize;
                    if self.union_seen[k] != self.union_epoch {
                        self.union_seen[k] = self.union_epoch;
                        self.delta_one(k, u, ch);
                    }
                }
                words &= words - 1;
            }
        }
    }
    /// The delta of a change of block word `word` alone, if every column that reads it is keyed by it: the
    /// records keyed by the old and by the new value, through each one's own column kernel (so the damage
    /// is the column delta's, record for record). `false`: the columns must answer one by one (some reader
    /// is not keyed by this word, moves with it, holds core-written state or reads `Time`; or the index
    /// was not asked for twice running since the last pass that wrote records).
    #[inline(never)]
    fn delta_word_keys(&mut self, word: usize, start: usize, end: usize, u: &U) -> bool {
        if self.no_fast {
            return false;
        }
        let ch = 1u64 << word;
        let at = match self.word_keys.iter().position(|w| w.word as usize == word) {
            Some(i) => i,
            None => {
                self.word_keys.push(WordKeys {
                    word: word as u8,
                    valid: 0,
                    asked: 0,
                    shape: STALE,
                    all_keyed: false,
                    keys: Vec::new(),
                });
                self.word_keys.len() - 1
            }
        };
        if self.word_keys[at].shape != self.shape {
            let ok = self.readers[start..end].iter().all(|&k| {
                let (route, st, col) = Self::column(&self.cols, k);
                st.space.reads() & ch == 0
                    && col.reads & ch & self.broad == 0
                    && !col.desc.timed && !col.desc.state
                    && route.fast != u16::MAX
                    && matches!(self.fast[route.fast as usize].fns.key, Some((w, ..)) if w as usize == word)
            });
            // Room for every record the readers can hold, taken here (cold: once per table shape), so that
            // building the index later (on a pointer move, typically) allocates nothing.
            let (sum, max) = self.readers[start..end].iter().fold((0, 0), |(s, m), &k| {
                let n = Self::column(&self.cols, k).2.nrec as usize;
                (s + n, m.max(n))
            });
            if ok {
                self.key_scratch.reserve(max);
            }
            let w = &mut self.word_keys[at];
            if ok {
                w.keys.clear();
                w.keys.reserve(sum);
            }
            (w.shape, w.all_keyed, w.valid, w.asked) = (self.shape, ok, 0, 0);
        }
        if !self.word_keys[at].all_keyed || self.broad & ch != 0 {
            return false;
        }
        let readers = (end - start) as u64;
        if self.word_keys[at].valid != self.wrote {
            if self.word_keys[at].asked != self.wrote {
                self.word_keys[at].asked = self.wrote;
                return false;
            }
            let mut keys = core::mem::take(&mut self.word_keys[at].keys);
            let mut scratch = core::mem::take(&mut self.key_scratch);
            keys.clear();
            for &k in &self.readers[start..end] {
                let (route, st, col) = Self::column(&self.cols, k);
                let Some((_, build, _)) = self.fast[route.fast as usize].fns.key else {
                    unreachable!("every reader is keyed (checked for this table shape)")
                };
                // SAFETY: the table is current; the kernel is this column's archetype's.
                unsafe { build(&st.ent(col), &mut scratch) };
                keys.extend(scratch.iter().map(|&(key, rec)| (key, k, rec)));
            }
            keys.sort_unstable();
            self.key_scratch = scratch;
            let w = &mut self.word_keys[at];
            (w.keys, w.valid) = (keys, self.wrote);
            self.fast_stats.key_builds += readers;
        }
        let (ov, nv) = (self.shown.w[word], u.w[word]);
        let keys = core::mem::take(&mut self.word_keys[at].keys);
        let mut n = 0u64;
        for v in [ov, nv] {
            #[cfg(feature = "mutant_keyed")]
            if v == ov && nv != ov {
                continue;
            }
            let from = keys.partition_point(|e| e.0 < v);
            for &(key, k, rec) in &keys[from..] {
                if key != v {
                    break;
                }
                let (route, st, col) = Self::column(&self.cols, k);
                let Some((_, _, delta)) = self.fast[route.fast as usize].fns.key else {
                    unreachable!("every reader is keyed (checked for this table shape)")
                };
                // SAFETY: the table is current and the index is of the records as they are (no pass that
                // wrote records since it was built); the kernel is the column's own, given that column's
                // one record by its key.
                let entry = st.ent(col);
                n +=
                    unsafe { delta(&entry, &[(key, rec)], &self.shown, u, &mut self.tiles) } as u64;
            }
            if nv == ov {
                break;
            }
        }
        self.word_keys[at].keys = keys;
        self.delta_stats.visited_incidences = n as usize;
        self.fast_stats.key_hits += readers;
        self.fast_stats.key_records += n;
        true
    }
    #[inline]
    fn column(cols: &[Column], k: u32) -> (Column, &Store, &crate::store::RawCol) {
        let route = cols[k as usize];
        // SAFETY: only a published table has entries. Its raw pointers come from the
        // stable store owners. Form the shared store and column borrows for this use.
        let st = unsafe { &*route.store };
        (route, st, &st.cols[route.index as usize])
    }

    #[inline(always)]
    fn delta_one(&mut self, k: usize, u: &U, ch: u64) {
        self.delta_stats.evaluated_columns += 1;
        let route = self.cols[k];
        // SAFETY: the table is current, and no view pass can run while it is borrowed here.
        let st = unsafe { &*route.store };
        let c = &st.cols[route.index as usize];
        let scroll = st.space.scroll;
        if scroll != crate::store::NO_WORD && ch >> (scroll & 63) & 1 != 0 {
            return;
        }
        if c.reads & ch & self.broad != 0 && !c.desc.state && !self.no_fast {
            self.damage_broad(st.space.clip);
        } else {
            let e = st.ent(c);
            if route.fast == u16::MAX
                || !Self::delta_fast(
                    &mut self.fast[route.fast as usize],
                    &e,
                    &self.shown,
                    u,
                    ch,
                    &mut self.tiles,
                    &mut self.fast_stats,
                    self.wrote,
                    self.no_fast,
                )
            {
                // SAFETY: the entry borrows the current column. Its kernel was instantiated for its archetype.
                unsafe { (e.delta)(&e, &self.shown, u, &mut self.tiles) };
            }
        }
        if c.desc.timed {
            st.resweep_col(c, u, &mut self.tiles);
        }
    }

    #[inline(never)]
    fn damage_broad(&mut self, clip: crate::expr::Rect) {
        #[cfg(not(feature = "mutant_broad"))]
        if self.broad_last != Some(clip) {
            self.tiles.mark(clip);
        }
        self.broad_last = Some(clip);
        self.fast_stats.broad_hits += 1;
    }

    #[inline(never)]
    #[allow(clippy::too_many_arguments)]
    fn delta_fast(
        f: &mut Fast,
        e: &ColEnt,
        old: &U,
        u: &U,
        ch: u64,
        tiles: &mut Tiles,
        stats: &mut FastStats,
        wrote: u64,
        no_fast: bool,
    ) -> bool {
        const PTR: u64 = 1 << U_PTR_X | 1 << U_PTR_Y;
        if no_fast {
            return false;
        }
        let rd = ch & e.reads;
        if let Some((word, build, delta)) = f.fns.key
            && rd == 1u64 << word
        {
            if f.key_valid != wrote {
                if f.key_asked != wrote {
                    f.key_asked = wrote;
                    stats.key_scans += 1;
                    return false;
                }
                // SAFETY: the table is current; the kernels are this column's archetype's.
                unsafe { build(e, &mut f.keys) };
                f.key_valid = wrote;
                stats.key_builds += 1;
            }
            // SAFETY: as above; the index is of the records as they are (no pass since it was built).
            let n = unsafe { delta(e, &f.keys, old, u, tiles) };
            stats.key_hits += 1;
            stats.key_records += n as u64;
            return true;
        }
        if let Some((boxes, delta)) = f.fns.near
            && rd & !PTR == 0
        {
            if f.box_valid != wrote {
                if f.box_asked != wrote {
                    f.box_asked = wrote;
                } else {
                    // SAFETY: as above.
                    unsafe { boxes(e, &mut f.boxes) };
                    f.box_valid = wrote;
                    stats.near_builds += 1;
                }
            }
            let b = (f.box_valid == wrote).then_some(&f.boxes[..]);
            // SAFETY: as above.
            let n = unsafe { delta(e, b, old, u, tiles) };
            stats.near_hits += 1;
            stats.near_records += n as u64;
            return true;
        }
        false
    }

    /// A frame at time `now`. Time advancing is not a turn: nothing is compared, no record is evaluated and no
    /// store is visited. Each sweep (a column with records in motion) is one mark.
    #[inline]
    pub fn tick(&mut self, now: u32) {
        self.clock = now;
        // Reduced motion: the raster shades every one-shot transition at its end by running `Time` far ahead of
        // the clock (loops read the `Env` bit instead). Nothing else knows.
        self.shown.w[U_TIME] = now.wrapping_add(if self.shown.w[U_ENV] & ENV_REDUCED_MOTION != 0 {
            REDUCED_MOTION_AHEAD
        } else {
            0
        });
        if !self.tiles.sweeps.is_empty() {
            if self.tiles.sweep_rects {
                self.tiles.tick_rects(self.shown.w[U_TIME], &self.shown);
            } else {
                self.tiles.tick(self.shown.w[U_TIME], &self.shown);
            }
        }
    }

    /// The pointer is at (x, y), for the records whose paint reads the pointer words. If none does (the
    /// usual case: hover goes through the pick and a handler) this is two stores.
    #[inline]
    pub fn pointer_words(&mut self, x: i32, y: i32) {
        let ch = ((self.shown.w[U_PTR_X] != x as u32) as u64) << U_PTR_X
            | ((self.shown.w[U_PTR_Y] != y as u32) as u64) << U_PTR_Y;
        if ch & self.reads != 0 {
            let mut u = self.shown;
            (u.w[U_PTR_X], u.w[U_PTR_Y]) = (x as u32, y as u32);
            self.delta(&u, ch);
        }
        (self.shown.w[U_PTR_X], self.shown.w[U_PTR_Y]) = (x as u32, y as u32);
    }

    /// After handlers ran: find what they changed and damage it.
    ///
    /// The block is compared with the copy the pixels were shaded with. If nothing differs and no relation was
    /// edited, that compare is the whole turn. Otherwise each column that reads a changed word differentiates
    /// its exposed pages, and the view is driven (with a sink that enters only lists) only if a relation was
    /// edited or a word that scrolls a list changed.
    #[inline]
    pub fn settle(&mut self, v: &V) {
        assert!(self.mounted, "the world has not been mounted");
        let mut scratch = U::default();
        let ch = self
            .shown
            .diff(v.gathered(&self.slots, &mut scratch), self.nwords)
            & !HOT;
        let journals = self.relations_changed();
        if ch != 0 || journals {
            self.settle_changed(v, ch, journals);
        }
    }
    // Re-learning can drive another description. Keep its stack frame off idle settles.
    #[inline(never)]
    fn settle_changed(&mut self, v: &V, ch: u64, journals: bool) {
        if self.changed(v, ch, journals) {
            self.learn(v);
        }
    }
    #[inline(always)]
    fn changed(&mut self, v: &V, ch: u64, journals: bool) -> bool {
        let u = &self.block(v);
        if !journals && ch & self.lists == 0 {
            // `settle` enters only for a changed word or journal, and `learn` only for a changed word.
            debug_assert_ne!(ch, 0);
            self.delta(u, ch);
            // `block` kept the current surface, raster and derived words. A word-only delta leaves them alone.
            self.shown.w[..APP_WORDS].copy_from_slice(&u.w[..APP_WORDS]);
            return false;
        }
        if self.pass_open {
            self.redescribe(v);
            return false;
        }
        if ch != 0 {
            self.delta(u, ch);
        }
        self.capture_hover();
        {
            let mut cx = Cx {
                tiles: &mut self.tiles,
                u,
                all: false,
                lists: 0,
                regroup: false,
                changed: 0,
                moved: 0,
                scale: f32::from_bits(u.w[U_SCALE]),
            };
            // as in `mount_pass`: stale while the view runs, as it was after it returns
            let shape = core::mem::replace(&mut self.shape, STALE);
            let cols = core::mem::take(&mut self.cols);
            self.pass_open = true;
            crate::el::track_inputs(&self.inputs, || {
                v.view(&mut Patch::<V, B> {
                    st: &mut self.root,
                    cx: &mut cx,
                    sub: 0,
                    _p: PhantomData,
                })
            });
            self.pass_open = false;
            if shape == self.root.structure_revision() {
                self.cols = cols;
                self.shape = shape;
            }
            self.wrote += (cx.changed != 0 || self.shape == STALE) as u64;
            self.lists |= cx.lists;
            self.revision = self.revision.wrapping_add(cx.changed);
            self.motion = self.motion.wrapping_add(cx.moved);
            if ch != 0 {
                self.shown = *u;
            }
            // windows may have moved: the focused row's place (or that it has none)
            self.refocus();
            // Only a description can change the set of observed relations or
            // the descriptors. Uniform-only delta stages neither.
            true
        }
    }

    /// Re-describing mode: the state is opaque, so the whole exposed view is described again and compared
    /// with the columns as it goes. The same `view` code as mount.
    pub fn redescribe(&mut self, v: &V) {
        assert!(self.mounted, "the world has not been mounted");
        let u = &self.block(v);
        let ch = self.shown.diff(u, self.nwords) & !HOT;
        if ch != 0 {
            self.delta(u, ch);
        }
        crate::probe::mark(crate::probe::ph::BLOCK);
        self.capture_hover();
        let mut cx = Cx {
            tiles: &mut self.tiles,
            u,
            all: true,
            lists: 0,
            regroup: false,
            changed: 0,
            moved: 0,
            scale: f32::from_bits(u.w[U_SCALE]),
        };
        let shape = core::mem::replace(&mut self.shape, STALE);
        let cols = core::mem::take(&mut self.cols);
        self.pass_open = true;
        crate::probe::mark(crate::probe::ph::ENTER);
        crate::el::track_inputs(&self.inputs, || {
            v.view(&mut Write::<V, B>::new(&mut self.root, &mut cx, false))
        });
        self.pass_open = false;
        if shape == self.root.structure_revision() {
            self.cols = cols;
            self.shape = shape;
        }
        self.lists |= cx.lists;
        let regroup = cx.regroup;
        // What was built from the records (a key index, page boxes) holds over a description that stored
        // nothing: `changed` counts every record stored again, every rect moved and every list window
        // moved, and a new column or store moves the structure revision.
        if cx.changed != 0 || shape != self.root.structure_revision() {
            self.wrote += 1;
        }
        self.revision = self.revision.wrapping_add(cx.changed);
        self.motion = self.motion.wrapping_add(cx.moved);
        crate::probe::mark(crate::probe::ph::LISTS);
        crate::probe::mark(crate::probe::ph::REDERIVE);
        if self.root.timed {
            self.root.resweep(u, &mut self.tiles);
        }
        if ch != 0 {
            self.shown = *u;
        }
        if regroup {
            // A fixed record moved over one of another column: the columns were grouped for an order that no
            // longer holds. The stores are made again from the view (cold: a description, as at mount).
            self.shape = STALE;
            self.cols.clear();
            *self.root = Store::new(1, self.root.space);
            self.tiles.sweeps.clear();
            self.mount(v);
        }
        self.learn(v);
        crate::probe::mark(crate::probe::ph::LEARN);
        self.refocus();
        crate::probe::mark(crate::probe::ph::REFOCUS);
    }

    // ---- events: the dataflow run backwards (`event.rs`) -----------------------------------------------------

    /// The place of record `rec` of column `col` of the store at `path`, numbering the world's columns in paint
    /// order (the column table's order): what the raster writes into `Pick`.
    pub fn place(&mut self, path: &Path, col: u32, rec: u32) -> u32 {
        assert!(col < 4096, "place column exceeds 12-bit capacity");
        assert!(
            rec < (1 << PLACE_REC_BITS),
            "place record exceeds 20-bit capacity"
        );
        self.columns();
        let Some(st) = self.root.at(path) else {
            return NONE;
        };
        match self.col_index(st, col) {
            Some(i) => i << PLACE_REC_BITS | rec,
            None => NONE,
        }
    }
    fn col_index(&self, st: &Store, col: u32) -> Option<u32> {
        let st = st as *const Store;
        self.cols
            .iter()
            .position(|entry| u32::from(entry.index) == col && core::ptr::eq(entry.store, st))
            .map(|i| i as u32)
    }
    /// The store, column and record a place names (the column table holds the store: no walk).
    ///
    /// The table is current for this world: every entry point brings it up to date before it dispatches, and
    /// nothing a handler does changes this world's store tree (only a settle does). The shape counter is
    /// process-wide, so another world growing must not make this one's places unreadable.
    pub fn at_place(&self, place: u32) -> Option<(&Store, u32, u32)> {
        // The vector is empty during a view pass and while waiting for a structural rebuild.
        let &Column {
            store, index: col, ..
        } = self.cols.get((place >> PLACE_REC_BITS) as usize)?;
        // SAFETY: an entry is available only in a published table. The world is shared-borrowed.
        let st = unsafe { &*store };
        let rec = place & ((1 << PLACE_REC_BITS) - 1);
        let c = st.cols.get(col as usize)?;
        // A store admits capacity one or a power-of-two ring. At capacity
        // one all records belong to row zero, including records on later pages.
        let row = rec & (st.cap - 1);
        (rec < c.nrec && st.row_live[row as usize / 64] & (1u64 << (row % 64)) != 0).then_some((
            st,
            u32::from(col),
            rec,
        ))
    }
    /// The column table, current (cold when the tree changed shape).
    fn columns(&mut self) -> bool {
        if self.shape == STALE {
            if self.pass_open {
                return false;
            }
            self.retable();
        }
        true
    }
    /// The path of a store of this world (cold: a walk).
    fn path_of(&self, want: *const Store) -> Option<Path> {
        fn go(st: &Store, want: *const Store, p: &mut Path) -> bool {
            if core::ptr::eq(st, want) {
                return true;
            }
            for (q, s) in st.subs.iter().enumerate() {
                p.at[p.n as usize] = q as u16;
                p.n += 1;
                if go(s, want, p) {
                    return true;
                }
                p.n -= 1;
            }
            false
        }
        let mut p = Path::default();
        go(&self.root, want, &mut p).then_some(p)
    }

    /// The places under (x, y) whose handlers hear one of `kinds`, bottom first (the last is the pick).
    fn chain(&mut self, x: i32, y: i32, kinds: u32) -> Vec<u32> {
        let mut out = self.take();
        self.chain_into(x, y, kinds, &mut out);
        out
    }
    /// A scratch buffer from the world's pool (allocated once, at the world's creation, for every event after).
    fn take(&mut self) -> Vec<u32> {
        let mut v = self.pool.pop().unwrap_or_default();
        v.clear();
        v
    }
    fn chain_into(&mut self, x: i32, y: i32, kinds: u32, out: &mut Vec<u32>) {
        out.clear();
        if !self.columns() {
            return;
        }
        let mut recs = core::mem::take(&mut self.buf);
        for &(k, hears) in &self.handlers {
            if hears & kinds == 0 {
                continue;
            }
            let route = self.cols[k as usize];
            // SAFETY: the table is current, and no view pass can run while it is borrowed here.
            let st = unsafe { &*route.store };
            let c = &st.cols[route.index as usize];
            let Some(o) = c.hover else { continue };
            let sv = crate::store::StoreView {
                space: &st.space,
                live: crate::store::Live {
                    cap: st.cap,
                    rows: &st.row_live,
                    nrec: c.nrec,
                },
            };
            recs.clear();
            if st.cap > 1 && !c.desc.moves && !st.spill && st.geo.2 > 0 && !self.no_fast {
                // A list of rows of one height, none of whose records leaves its row's band or moves by
                // its paint: the records under the point are those of the one row the point is in. The row
                // is arithmetic; its records in this column are one per site of the column.
                let clip = st.space.clip;
                if (x >= clip.x0) & (x < clip.x1) & (y >= clip.y0) & (y < clip.y1) {
                    let org = st.space.origin(&self.shown);
                    let (px, py) = (x - org.0, y - org.1);
                    let ly = py - st.geo.1;
                    if ly >= 0 {
                        let r = (ly / st.geo.2) as u32;
                        if (r >= st.lo) & (r < st.hi) {
                            let slot = st.slot_of(r);
                            for s in st.sites.iter().filter(|s| s.col == u32::from(route.index)) {
                                // SAFETY: a record's site of this store (its column is `j`, not a hole);
                                // the slot is under the store's capacity; the table is current.
                                let b = unsafe { s.rect(slot) };
                                if (px >= b.x0) & (px < b.x1) & (py >= b.y0) & (py < b.y1) {
                                    let rec = s.base + slot;
                                    if c.rec_init(rec) {
                                        recs.push(rec);
                                    }
                                }
                            }
                        }
                    }
                }
            } else {
                // SAFETY: `hit_all` was instantiated for the archetype the column was created with.
                unsafe { (o.hit_all)(c, &sv, &self.shown, x, y, &mut recs) };
            }
            out.extend(recs.iter().map(|&r| k << PLACE_REC_BITS | r));
        }
        self.buf = recs;
    }
    /// Give a scratch buffer back to the pool.
    fn done(&mut self, v: Vec<u32>) {
        self.pool.push(v);
    }

    /// Run the handler of `place` that hears `e`, if it has one.
    fn call(&self, v: &mut V, place: u32, e: &Event) -> bool {
        // V is invariant on World and Sink<V> instantiates these handlers.
        assert!(self.mounted, "the world has not been mounted");
        let Some((st, col, rec)) = self.at_place(place) else {
            return false;
        };
        let Some((handler, captures, record)) = st.cols[col as usize].handler_data(rec, e.kind)
        else {
            return false;
        };
        // SAFETY: the lookup admitted an initialized record and its capture lane. The handler was
        // instantiated for this archetype, its captures, and the world's invariant view type V.
        unsafe { (handler.call)(v as *mut V as *mut (), captures, record, e) }
    }
    /// The first place of `chain` (topmost first) whose handler consumes `e`.
    fn bubble(&self, v: &mut V, chain: impl Iterator<Item = u32>, e: &Event) -> Dispatch {
        for p in chain {
            if self.call(v, p, e) {
                return Dispatch {
                    kind: e.kind,
                    place: p,
                    consumed: true,
                };
            }
        }
        Dispatch {
            kind: e.kind,
            place: NONE,
            consumed: false,
        }
    }

    /// Write a core word (focus, press, environment) as the raster would see it, damaging what reads it.
    fn core_word(&mut self, slot: usize, val: u32) {
        if self.shown.w[slot] == val {
            return;
        }
        if self.reads >> slot & 1 != 0 {
            let mut u = self.shown;
            u.w[slot] = val;
            self.delta(&u, 1 << slot);
        }
        self.shown.w[slot] = val;
    }

    /// The platform's settings changed (`event::ENV_*` bits). Reduced motion takes effect at the next frame.
    pub fn set_env(&mut self, bits: u32) {
        self.core_word(U_ENV, bits);
    }

    /// The pointer moved to (x, y): the pick, then hover left and entered (the chain's symmetric difference),
    /// or, while a drag handler holds the pointer, the drag.
    pub fn pointer(&mut self, v: &mut V, x: i32, y: i32) -> Dispatch {
        self.ptr = Some((x, y));
        // the words a paint may read the pointer from move with it, in the same operation
        self.pointer_words(x, y);
        if let Some(h) = self.held
            && h.drag != NONE
        {
            let e = Event {
                kind: kind::DRAG,
                ev: Ev {
                    dx: (x - h.at.0) as f32,
                    dy: (y - h.at.1) as f32,
                    ..Ev::at(x as f32, y as f32)
                },
                text: "",
            };
            let consumed = self.call(v, h.drag, &e);
            return Dispatch {
                kind: kind::DRAG,
                place: h.drag,
                consumed,
            };
        }
        self.repick(v)
    }
    /// The pick at the pointer as it stands, and hover brought up to date: after a move, or after a turn that
    /// moved content under a still pointer (what the raster's pick in the frame being drawn delivers).
    pub fn repick(&mut self, v: &mut V) -> Dispatch {
        let Some((x, y)) = self.ptr else {
            return Dispatch::NOTHING;
        };
        crate::probe::mark(crate::probe::ph::BLOCK);
        let all = self.chain(x, y, kind::POINTER);
        crate::probe::mark(crate::probe::ph::CHAIN);
        let d = self.picked(v, all.last().copied().unwrap_or(NONE), &all);
        self.done(all);
        crate::probe::mark(crate::probe::ph::PICKED);
        d
    }
    /// A pick that arrived from the raster with a frame: `place` is the `Pick` word.
    pub fn accept_pick(&mut self, v: &mut V, token: Place) -> Dispatch {
        self.columns();
        if self.at_token(token).is_none() {
            return Dispatch::NOTHING;
        }
        let place = token.packed;
        let all = match self.ptr {
            Some((x, y)) if place != NONE => self.chain(x, y, kind::POINTER),
            _ => self.take(),
        };
        let d = self.picked(v, place, &all);
        self.done(all);
        d
    }
    fn picked(&mut self, v: &mut V, pick: u32, all: &[u32]) -> Dispatch {
        self.core_word(U_PICK, pick);
        let mut next = self.take();
        let mut stateful = 0;
        for &p in all {
            let kinds = self.kinds(p);
            if kinds & kind::HOVER != 0 {
                next.push(p);
                stateful |= kinds & HOVER_STATE;
            }
        }
        // Places whose slot has been given to another row since they were entered: the item that was entered
        // is gone from under the pointer, and the slot holds another row's record and captures. It hears
        // its leave with the record and the captures it was entered with (kept copies), topmost first, and
        // is no longer hovered; whatever is in the slot now is entered below like any other place.
        let (px, py) = self.ptr.map_or((-1.0, -1.0), |(x, y)| (x as f32, y as f32));
        let mut i = self.hov_kept.len();
        while i > 0 {
            i -= 1;
            if self.stamp_at(self.hov[i]) != self.hov_what[i] {
                let k = self.hov_kept[i];
                self.hov.remove(i);
                self.hov_what.remove(i);
                self.hov_kept.remove(i);
                if let Some(o) = k.ons {
                    let e = Event {
                        kind: kind::HOVER,
                        ev: Ev {
                            on: false,
                            ..Ev::at(px, py)
                        },
                        text: "",
                    };
                    let (rec, caps) = if k.rec == u32::MAX {
                        let saved = &self.hov_aligned[k.caps as usize];
                        let rec = saved.ptr.as_ptr().cast_const();
                        // SAFETY: the capture offset is aligned and within this saved allocation.
                        let caps = unsafe { rec.add(saved.captures) };
                        (rec, caps)
                    } else {
                        let rec = self.hov_bytes[k.rec as usize..].as_ptr().cast::<u8>();
                        let caps = if o.caps == 0 {
                            core::ptr::without_provenance(o.caps_align as usize)
                        } else {
                            self.hov_bytes[k.caps as usize..].as_ptr().cast::<u8>()
                        };
                        (rec, caps)
                    };
                    // SAFETY: these are the kept, aligned copies for this handler and view type.
                    // The handler only reads the initialized fields through their original types.
                    unsafe { (o.call)(v as *mut V as *mut (), caps, rec, &e) };
                }
            }
        }
        if next == self.hov {
            // the usual move: the hovered set is what it was
            self.done(next);
            return Dispatch {
                kind: kind::HOVER,
                place: pick,
                consumed: false,
            };
        }
        let old = core::mem::replace(&mut self.hov, next);
        self.hov_what.clear();
        self.hov_kept.clear();
        self.hov_bytes.clear();
        self.hov_aligned.clear();
        if stateful != 0 {
            self.capture_hover();
        }
        let (x, y) = (px, py);
        let mut out = Dispatch {
            kind: kind::HOVER,
            place: NONE,
            consumed: false,
        };
        // left: the old set's records not under the pointer now, topmost first
        for i in (0..old.len()).rev() {
            let p = old[i];
            if !self.hov.contains(&p) {
                self.call(
                    v,
                    p,
                    &Event {
                        kind: kind::HOVER,
                        ev: Ev {
                            on: false,
                            ..Ev::at(x, y)
                        },
                        text: "",
                    },
                );
            }
        }
        // entered: the new set's records that were not under it, topmost first
        for i in (0..self.hov.len()).rev() {
            let p = self.hov[i];
            if !old.contains(&p)
                && self.call(
                    v,
                    p,
                    &Event {
                        kind: kind::HOVER,
                        ev: Ev::at(x, y),
                        text: "",
                    },
                )
                && out.place == NONE
            {
                out = Dispatch {
                    kind: kind::HOVER,
                    place: p,
                    consumed: true,
                };
            }
        }
        self.done(old);
        out
    }
    /// Save what the hovered places denote before a write can replace them. Stateful
    /// hover records are saved on entry because their delta may write before a pass.
    #[inline(always)]
    fn capture_hover(&mut self) {
        if !self.hov.is_empty() && self.hov_kept.is_empty() {
            self.capture_hover_lanes();
        }
    }
    #[inline(never)]
    fn capture_hover_lanes(&mut self) {
        for i in 0..self.hov.len() {
            let p = self.hov[i];
            self.hov_what.push(self.stamp_at(p));
            self.keep(p);
        }
    }

    /// Identity of the column allocation and of the item occupying this slot.
    fn stamp_at(&self, place: u32) -> u64 {
        let Some((st, col, rec)) = self.at_place(place) else {
            return 0;
        };
        let stamp = st
            .row_stamp
            .get((rec & (st.cap - 1)) as usize)
            .copied()
            .unwrap_or(0);
        (u64::from(st.cols[col as usize].id) << 32) | u64::from(stamp)
    }

    /// Keep the initialized record and captures at `place`, including any padding in their storage.
    fn keep(&mut self, place: u32) {
        let mut k = Kept {
            ons: None,
            rec: u32::MAX,
            caps: u32::MAX,
        };
        if let Some((st, col, rec)) = self.at_place(place) {
            let c = &st.cols[col as usize];
            if let Some((handler, captures, record)) = c.handler_data(rec, kind::HOVER) {
                let (rsize, csize) = (c.desc.rec_size, handler.caps as usize);
                let align = c.desc.rec_align.max(handler.caps_align as usize);
                k.ons = Some(handler);
                if align <= align_of::<u128>() {
                    let r0 = self.hov_bytes.len();
                    let c0 = r0 + rsize.div_ceil(16).max(1);
                    let mut bytes = core::mem::take(&mut self.hov_bytes);
                    bytes.resize(c0 + csize.div_ceil(16), core::mem::MaybeUninit::uninit());
                    // SAFETY: `handler_data` admitted the initialized lanes. The arena stores
                    // MaybeUninit words so their padding can be copied and survive reallocation.
                    unsafe {
                        record.copy_to_nonoverlapping(bytes[r0..].as_mut_ptr().cast(), rsize);
                        if csize != 0 {
                            captures.copy_to_nonoverlapping(bytes[c0..].as_mut_ptr().cast(), csize);
                        }
                    }
                    self.hov_bytes = bytes;
                    k.rec = u32::try_from(r0).expect("hover record arena exhausted");
                    k.caps = u32::try_from(c0).expect("hover capture arena exhausted");
                } else {
                    let offset = rsize.max(1).next_multiple_of(align);
                    let size = offset
                        .checked_add(csize.max(1))
                        .expect("hover copy too large");
                    let layout = std::alloc::Layout::from_size_align(size, align)
                        .expect("hover copy layout");
                    // SAFETY: `layout` is nonempty. Copy the admitted lanes into disjoint aligned
                    // ranges. AlignedHover owns the untyped bytes and releases them with this layout.
                    let saved = unsafe {
                        let ptr = core::ptr::NonNull::new(std::alloc::alloc(layout))
                            .unwrap_or_else(|| std::alloc::handle_alloc_error(layout));
                        record.copy_to_nonoverlapping(ptr.as_ptr(), rsize);
                        if csize != 0 {
                            captures.copy_to_nonoverlapping(ptr.as_ptr().add(offset), csize);
                        }
                        AlignedHover {
                            ptr,
                            layout,
                            captures: offset,
                        }
                    };
                    k.caps = u32::try_from(self.hov_aligned.len())
                        .expect("hover capture arena exhausted");
                    self.hov_aligned.push(saved);
                }
            }
        }
        self.hov_kept.push(k);
    }
    /// The handler mask of a hit from `chain_into`, which already admitted its record.
    fn kinds(&self, place: u32) -> u32 {
        self.cols[(place >> PLACE_REC_BITS) as usize].kinds
    }

    /// A button went down at (x, y): focus moves to the nearest focusable record under it (or leaves, as the
    /// DOM blurs on the background), `Press` and `PressAt` are written, the press handler runs, and a drag
    /// handler captures the pointer.
    pub fn pointer_down(&mut self, v: &mut V, x: i32, y: i32, button: Button) -> Dispatch {
        // one walk under the pointer serves the pick, hover, focus, the press and the drag
        self.ptr = Some((x, y));
        let chain = self.chain(x, y, kind::POINTER | kind::FOCUSABLE);
        let mut ptr = self.take();
        for &p in &chain {
            if self.kinds(p) & kind::POINTER != 0 {
                ptr.push(p);
            }
        }
        self.picked(v, ptr.last().copied().unwrap_or(NONE), &ptr);
        self.done(ptr);
        if button == Button::Primary {
            let f = chain
                .iter()
                .rev()
                .copied()
                .find(|&p| self.kinds(p) & kind::FOCUSABLE != 0);
            self.focus_place(v, f);
        }
        let top = chain
            .iter()
            .rev()
            .copied()
            .find(|&p| self.kinds(p) & kind::POINTER != 0)
            .unwrap_or(NONE);
        let drag = chain
            .iter()
            .rev()
            .copied()
            .find(|&p| self.kinds(p) & kind::DRAG != 0)
            .unwrap_or(NONE);
        let what = if top == NONE { None } else { self.logical(top) };
        self.held = Some(Held {
            place: top,
            what,
            drag,
            at: (x, y),
            button,
        });
        if button == Button::Primary {
            self.core_word(U_PRESS, top);
            self.core_word(U_PRESS_AT, (x as u32 & 0xffff) << 16 | (y as u32 & 0xffff));
        }
        let e = Event {
            kind: kind::PRESS,
            ev: Ev {
                button,
                ..Ev::at(x as f32, y as f32)
            },
            text: "",
        };
        let d = self.bubble(v, chain.iter().rev().copied(), &e);
        self.done(chain);
        d
    }

    /// The button came up at (x, y): a drag ends (its handler sees `ev.on == false`); otherwise, if the pointer
    /// is still over the record pressed, it is activated (the click handler nearest to it under the pointer).
    pub fn pointer_up(&mut self, v: &mut V, x: i32, y: i32, button: Button) -> Dispatch {
        self.columns();
        let Some(h) = self.held.take() else {
            return Dispatch::NOTHING;
        };
        if h.button == Button::Primary {
            self.core_word(U_PRESS, NONE);
        }
        if h.drag != NONE {
            let e = Event {
                kind: kind::DRAG,
                ev: Ev {
                    dx: (x - h.at.0) as f32,
                    dy: (y - h.at.1) as f32,
                    on: false,
                    ..Ev::at(x as f32, y as f32)
                },
                text: "",
            };
            let consumed = self.call(v, h.drag, &e);
            self.pointer(v, x, y);
            return Dispatch {
                kind: kind::DRAG,
                place: h.drag,
                consumed,
            };
        }
        // one walk under the pointer serves the pick, hover and the click
        self.ptr = Some((x, y));
        let chain = self.chain(x, y, kind::POINTER);
        self.picked(v, chain.last().copied().unwrap_or(NONE), &chain);
        // the pressed place must still show what was pressed: a scroll or an edit in between may have put
        // another row in its slot, and that row was not pressed
        let same = h.what.is_some_and(|w| {
            self.logical(h.place)
                .is_some_and(|n| (n.path, n.site, n.row) == (w.path, w.site, w.row))
        });
        let d = match chain.iter().position(|&p| p == h.place) {
            Some(i) if button == Button::Primary && h.place != NONE && same => {
                let e = Event {
                    kind: kind::CLICK,
                    ev: Ev::at(x as f32, y as f32),
                    text: "",
                };
                self.bubble(v, chain[..=i].iter().rev().copied(), &e)
            }
            _ => Dispatch::NOTHING,
        };
        self.done(chain);
        d
    }

    /// A press and a release at (x, y).
    pub fn click(&mut self, v: &mut V, x: i32, y: i32) -> Dispatch {
        self.pointer_down(v, x, y, Button::Primary);
        self.pointer_up(v, x, y, Button::Primary)
    }

    /// A key went down. Tab and Shift+Tab move focus in paint order; otherwise the focused record's key handler,
    /// then those of the records with key handlers it lies inside, innermost first (with nothing focused: those
    /// that hold the window's origin, the root's included).
    pub fn key_down(&mut self, v: &mut V, key: Key, mods: Mods) -> Dispatch {
        self.columns();
        if key == Key::Tab {
            return self.tab(v, mods.shift);
        }
        let e = Event {
            kind: kind::KEY,
            ev: Ev {
                key: Some(key),
                mods,
                ..Ev::at(0.0, 0.0)
            },
            text: "",
        };
        let focused = self.shown.w[U_FOCUS];
        if focused != NONE {
            if self.call(v, focused, &e) {
                return Dispatch {
                    kind: kind::KEY,
                    place: focused,
                    consumed: true,
                };
            }
            // Enter or Space on a focused record that a click activates: its own behaviour, before any record it
            // lies inside hears the key
            if matches!(key, Key::Enter | Key::Char(' ')) {
                let c = Event {
                    kind: kind::CLICK,
                    ..e
                };
                if self.call(v, focused, &c) {
                    return Dispatch {
                        kind: kind::CLICK,
                        place: focused,
                        consumed: true,
                    };
                }
            }
        }
        let (fx, fy) = self.center(focused).unwrap_or((0, 0));
        let mut chain = self.chain(fx, fy, kind::KEY);
        chain.retain(|&p| p != focused);
        let d = self.bubble(v, chain.iter().rev().copied(), &e);
        self.done(chain);
        d
    }

    /// Text typed (an IME commit, or a printable key's text) goes to the focused record's text handler.
    pub fn text(&mut self, v: &mut V, s: &str) -> Dispatch {
        self.columns();
        let focused = self.shown.w[U_FOCUS];
        let e = Event {
            kind: kind::TEXT,
            ev: Ev::at(0.0, 0.0),
            text: s,
        };
        let consumed = focused != NONE && self.call(v, focused, &e);
        Dispatch {
            kind: kind::TEXT,
            place: if consumed { focused } else { NONE },
            consumed,
        }
    }

    /// The wheel turned by (dx, dy) lines at (x, y): the wheel handlers under the pointer first (topmost first,
    /// one that returns `true` consumes it); otherwise the innermost space under the pointer whose origin reads a
    /// scroll word moves, within its lists' extent: the space is its own scroller. One line is a row of its
    /// first list.
    pub fn wheel(&mut self, v: &mut V, x: i32, y: i32, dx: f32, dy: f32) -> Dispatch {
        self.ptr = Some((x, y));
        let chain = self.chain(x, y, kind::WHEEL);
        let e = Event {
            kind: kind::WHEEL,
            ev: Ev {
                dx,
                dy,
                ..Ev::at(x as f32, y as f32)
            },
            text: "",
        };
        let d = self.bubble(v, chain.iter().rev().copied(), &e);
        self.done(chain);
        if d.consumed {
            return d;
        }
        fn scroller(st: &Store, x: i32, y: i32) -> Option<&Store> {
            let c = st.space.clip;
            if x < c.x0 || x >= c.x1 || y < c.y0 || y >= c.y1 {
                return None;
            }
            let inner = st.subs.iter().find_map(|s| scroller(s, x, y));
            // a list inherits its scope's space; the scope is the scroller
            inner.or((st.space.scroll != crate::store::NO_WORD && st.src.is_none()).then_some(st))
        }
        let Some(st) = scroller(&self.root, x, y) else {
            return Dispatch {
                kind: kind::WHEEL,
                place: NONE,
                consumed: false,
            };
        };
        // the offset's word, and whether it is the application's own (an `f32` of points) or device pixels
        let points = st.space.scroll & crate::store::POINTS != 0;
        let word = st.space.scroll as usize & 63;
        let scale = f32::from_bits(self.shown.w[U_SCALE]);
        let lists = st.subs.iter().filter(|s| s.src.is_some());
        let (extent, line) = lists.fold((0i32, 0i32), |(e, l), s| {
            (
                e.max(s.geo.1 + s.n as i32 * s.geo.2),
                if l == 0 { s.geo.2 } else { l },
            )
        });
        let max = (extent - (st.space.clip.y1 - st.space.clip.y0)).max(0);
        let Some(u) = v.block_mut() else {
            return Dispatch {
                kind: kind::WHEEL,
                place: NONE,
                consumed: false,
            };
        };
        let at = if points {
            (f32::from_bits(u.w[word]) * scale).round() as i32
        } else {
            u.w[word] as i32
        };
        let to = (at + (dy * line.max(1) as f32) as i32).clamp(0, max);
        if to != at {
            u.w[word] = if points {
                (to as f32 / scale).to_bits()
            } else {
                to as u32
            };
        }
        Dispatch {
            kind: kind::WHEEL,
            place: NONE,
            consumed: to != at,
        }
    }

    /// Bring every scrolled space's offset back within its content: after a resize or an edit that made the
    /// content shorter than where it was scrolled to, the port would otherwise show nothing past the last
    /// row. The limit is the wheel's (the lists' extent less the port's height). Returns whether an offset
    /// was written (into the view's block, as the wheel writes it: the caller settles). Cold: a walk of the
    /// stores.
    pub fn clamp_scroll(&mut self, v: &mut V) -> bool {
        fn go(st: &Store, scale: f32, u: &mut U, any: &mut bool) {
            if st.space.scroll != crate::store::NO_WORD && st.src.is_none() {
                let points = st.space.scroll & crate::store::POINTS != 0;
                let word = st.space.scroll as usize & 63;
                let lists = st.subs.iter().filter(|s| s.src.is_some());
                let (extent, some) = lists.fold((0i32, false), |(e, _), s| {
                    (e.max(s.geo.1 + s.n as i32 * s.geo.2), true)
                });
                if some {
                    let max = (extent - (st.space.clip.y1 - st.space.clip.y0)).max(0);
                    let at = if points {
                        (f32::from_bits(u.w[word]) * scale).round() as i32
                    } else {
                        u.w[word] as i32
                    };
                    if at > max {
                        u.w[word] = if points {
                            (max as f32 / scale).to_bits()
                        } else {
                            max as u32
                        };
                        *any = true;
                    }
                }
            }
            for s in &st.subs {
                go(s, scale, u, any);
            }
        }
        let scale = f32::from_bits(self.shown.w[U_SCALE]);
        let Some(u) = v.block_mut() else {
            return false;
        };
        let mut any = false;
        go(&self.root, scale, u, &mut any);
        any
    }

    /// Activate a place as a click would, with no pointer: what an assistive technology's "press" does. The
    /// place's own click handler runs, or the nearest under its centre.
    pub fn activate(&mut self, v: &mut V, place: u32) -> Dispatch {
        self.columns();
        let e = Event {
            kind: kind::CLICK,
            ev: Ev::at(0.0, 0.0),
            text: "",
        };
        if self.call(v, place, &e) {
            return Dispatch {
                kind: kind::CLICK,
                place,
                consumed: true,
            };
        }
        let Some((x, y)) = self.center(place) else {
            return Dispatch::NOTHING;
        };
        let chain = self.chain(x, y, kind::CLICK);
        let d = self.bubble(v, chain.iter().rev().copied(), &e);
        self.done(chain);
        d
    }

    /// Scroll the list at `path` so that row `r` is exposed (as little as needed): what an assistive
    /// technology's "scroll into view" does. Writes the scroll word of the list's space; the next settle
    /// exposes the row. Returns whether the word changed.
    pub fn scroll_into_view(&mut self, v: &mut V, path: &Path, r: u32) -> bool {
        let Some(st) = self.root.at(path) else {
            return false;
        };
        let (sp, (_, y, h)) = (st.space, st.geo);
        if sp.scroll == crate::store::NO_WORD || h == 0 {
            return false;
        }
        let Some(u) = v.block_mut() else { return false };
        let at = u.w[sp.scroll as usize] as i32;
        let view = sp.clip.y1 - sp.clip.y0;
        let (top, bot) = (y + r as i32 * h, y + (r as i32 + 1) * h);
        let to = if top < at {
            top
        } else if bot > at + view {
            bot - view
        } else {
            at
        };
        u.w[sp.scroll as usize] = to as u32;
        to != at
    }

    /// The centre of a place's record on screen.
    fn center(&self, place: u32) -> Option<(i32, i32)> {
        let (st, col, rec) = self.at_place(place)?;
        let r = rect_of(st, col, rec)?.shift(st.space.origin(&self.shown));
        Some(((r.x0 + r.x1) / 2, (r.y0 + r.y1) / 2))
    }

    /// The focusable places in paint order, exposed records only.
    fn focusables(&mut self) -> Vec<u32> {
        self.columns();
        let mut out = self.take();
        let mut recs = self.take();
        for (i, route) in self.cols.iter().enumerate() {
            // SAFETY: the table is current, and no view pass can run while it is borrowed here.
            let st = unsafe { &*route.store };
            let c = &st.cols[route.index as usize];
            if c.hover.is_none_or(|o| o.kinds & kind::FOCUSABLE == 0) {
                continue;
            }
            let live = crate::store::Live {
                cap: st.cap,
                rows: &st.row_live,
                nrec: c.nrec,
            };
            recs.clear();
            for pg in 0..c.npages {
                let mut m = live.page(pg as usize);
                while m != 0 {
                    recs.push(pg * 64 + m.trailing_zeros());
                    m &= m - 1;
                }
            }
            // a list's records in row order, not ring order
            if st.cap > 1 {
                recs.sort_unstable_by_key(|&r| (row_of(st, r), r / st.cap));
            }
            out.extend(recs.iter().map(|&r| (i as u32) << PLACE_REC_BITS | r));
        }
        self.done(recs);
        out
    }
    /// Move focus to the next (or previous) focusable record in paint order, wrapping.
    pub fn tab(&mut self, v: &mut V, back: bool) -> Dispatch {
        let all = self.focusables();
        if all.is_empty() {
            self.done(all);
            return Dispatch {
                kind: kind::FOCUS,
                place: NONE,
                consumed: false,
            };
        }
        let cur = all.iter().position(|&p| p == self.shown.w[U_FOCUS]);
        let n = all.len();
        let next = match (cur, back) {
            (None, false) => 0,
            (None, true) => n - 1,
            (Some(i), false) => (i + 1) % n,
            (Some(i), true) => (i + n - 1) % n,
        };
        let p = all[next];
        self.done(all);
        self.focus_place(v, Some(p));
        Dispatch {
            kind: kind::FOCUS,
            place: p,
            consumed: true,
        }
    }

    /// Focus a place (`None`: nothing). The record's focus handler hears it; so does the one that lost focus.
    pub fn focus_place(&mut self, v: &mut V, place: Option<u32>) {
        self.columns();
        let old = self.shown.w[U_FOCUS];
        let new = place.unwrap_or(NONE);
        if old == new {
            return;
        }
        self.focus = place.and_then(|p| self.logical(p));
        self.core_word(U_FOCUS, new);
        if old != NONE {
            self.call(
                v,
                old,
                &Event {
                    kind: kind::FOCUS,
                    ev: Ev {
                        on: false,
                        ..Ev::at(0.0, 0.0)
                    },
                    text: "",
                },
            );
        }
        if new != NONE {
            self.call(
                v,
                new,
                &Event {
                    kind: kind::FOCUS,
                    ev: Ev::at(0.0, 0.0),
                    text: "",
                },
            );
        }
    }
    /// The focused record as (store, site, row), or `None` when nothing is focused.
    pub fn focused(&self) -> Option<(Path, u32, u32)> {
        self.focus.map(|f| (f.path, f.site, f.row))
    }
    fn logical(&self, place: u32) -> Option<Focused> {
        let (st, col, rec) = self.at_place(place)?;
        let path = self.path_of(st)?;
        let k = rec / st.cap;
        let site = st
            .sites
            .iter()
            .enumerate()
            .filter(|(_, s)| s.col == col)
            .nth(k as usize)?
            .0 as u32;
        Some(Focused {
            path,
            site,
            row: row_of(st, rec),
            seen: st.seen,
        })
    }
    /// After a pass that may have moved a window: where the focused row is now, or that it is not exposed
    /// (`Focus` holds `NONE` until it comes back). Nothing to do when nothing is focused.
    /// After a pass: the pressed row follows its item through the inserts and removes the pass replayed, as
    /// the focused row does. If its item was removed, or the edits are more than the journal kept, nothing
    /// is pressed any more (a release then activates nothing): a press is on an item, and the row that
    /// later has its index, or its slot, was not pressed.
    #[inline(never)]
    fn rehold(&mut self, mut h: Held) {
        let Some(mut w) = h.what else { return };
        self.columns();
        let Some(st) = self.root.at(&w.path) else {
            return;
        };
        if st.src.is_none() || st.seen == w.seen {
            return;
        }
        let mut alive = st.moved_from == w.seen && st.moved_ok;
        if alive {
            for e in &st.moved {
                match *e {
                    crate::sink::Edit::Insert(at) if at <= w.row => w.row += 1,
                    crate::sink::Edit::Remove(at) if at < w.row => w.row -= 1,
                    crate::sink::Edit::Remove(at) if at == w.row => {
                        alive = false;
                        break;
                    }
                    _ => {}
                }
            }
        }
        w.seen = st.seen;
        h.what = alive.then_some(w);
        if alive {
            // where the row is now (the same record if the list kept it, another slot if it wrote it again)
            h.place = (|| {
                let s = st.sites.get(w.site as usize)?;
                if !(w.row >= st.lo && w.row < st.hi) {
                    return None;
                }
                let rec = s.base + st.slot_of(w.row);
                let i = self.col_index(st, s.col)?;
                Some(i << PLACE_REC_BITS | rec)
            })()
            .unwrap_or(NONE);
        }
        self.held = Some(h);
    }
    #[inline]
    fn refocus(&mut self) {
        if let Some(h) = self.held {
            self.rehold(h);
        }
        let Some(mut f) = self.focus else { return };
        self.columns();
        let Some(st) = self.root.at(&f.path) else {
            self.core_word(U_FOCUS, NONE);
            return;
        };
        // the focused row follows its item through the inserts and removes the last pass replayed
        if st.src.is_some() && st.seen != f.seen {
            if st.moved_from == f.seen && st.moved_ok {
                for e in &st.moved {
                    match *e {
                        crate::sink::Edit::Insert(at) if at <= f.row => f.row += 1,
                        crate::sink::Edit::Remove(at) if at < f.row => f.row -= 1,
                        crate::sink::Edit::Remove(at) if at == f.row => {
                            // the focused item is gone: focus is nowhere
                            self.focus = None;
                            self.core_word(U_FOCUS, NONE);
                            return;
                        }
                        _ => {}
                    }
                }
            }
            f.seen = st.seen;
            self.focus = Some(f);
        }
        let place = (|| {
            let s = st.sites.get(f.site as usize)?;
            let exposed = st.cap == 1 || (f.row >= st.lo && f.row < st.hi);
            if !exposed {
                return None;
            }
            let rec = s.base + st.slot_of(f.row);
            let old_column = self.shown.w[U_FOCUS] >> PLACE_REC_BITS;
            let column = if self.cols.get(old_column as usize).is_some_and(|entry| {
                core::ptr::eq(entry.store, st) && u32::from(entry.index) == s.col
            }) {
                old_column
            } else {
                self.col_index(st, s.col)?
            };
            Some(column << PLACE_REC_BITS | rec)
        })()
        .unwrap_or(NONE);
        self.core_word(U_FOCUS, place);
    }

    /// Paint everything through the backend with the block as last settled.
    pub fn paint(&self, b: &mut B) {
        if !self.pass_open {
            self.root.paint(&self.shown, b);
        }
    }
}

/// The row a record of store `st` shows: 0 in a fixed shape; in a list, the row of the window at its slot.
fn row_of(st: &Store, rec: u32) -> u32 {
    if st.cap == 1 {
        return 0;
    }
    st.row_at_slot(rec % st.cap)
}
/// A record's rect in its store's space.
fn rect_of(st: &Store, col: u32, rec: u32) -> Option<crate::store::Rect> {
    let base = rec / st.cap * st.cap;
    let s = st.sites.iter().find(|s| s.col == col && s.base == base)?;
    // SAFETY: a site of this store; the slot is under its capacity; site pointers are current.
    Some(unsafe { s.rect(rec % st.cap) })
}
