//! The per-turn draft a description is written into (`designs/reactivity/final/02-elements.md` §2): an
//! [`El`](super::El) is a cursor to one [`DNode`] here, and everything a node points at (its operations, its
//! wire and route requests, copied text) lives in the bump store beside it.
//!
//! One arena per describing thread. A description is built (`view`), walked once by `mount`, `update` or
//! `lower` (`walk.rs`), and then the arena is rewound: the next description reuses the same memory, so a
//! steady-state turn allocates nothing.
//!
//! What keeps a rewound arena safe is the turn number. Every node carries the number of the turn it was
//! written in and every `El` carries a copy; a rewind zeroes the numbers of the nodes it retires and moves to
//! the next turn, so a handle kept past its turn never matches again and every use of it panics where it is
//! used. That check reads the node, so node memory is never returned to the allocator: the chunks are leaked
//! with the thread (a few hundred bytes per node of the largest description the thread built), and the
//! thread-local has no destructor, so the check is valid for as long as the thread runs.
use crate::input::{Key, RouteFn};
use crate::layout::Spec;
use crate::rt::PatchFn;
use std::any::TypeId;
use std::cell::{Cell, UnsafeCell};
use std::mem::{ManuallyDrop, MaybeUninit};
use std::ptr::{self, NonNull};

/// Node kinds.
pub(crate) const K_BOX: u8 = 1;
pub(crate) const K_TEXT: u8 = 2;
/// A dynamic part: each child is one item (`describe::mount_dyn`).
pub(crate) const K_GROUP: u8 = 3;
/// Children in flow with no node of their own (a tuple as one item of a dynamic part).
pub(crate) const K_FRAG: u8 = 4;
/// A one-line text input bound to a `Signal<String>` (`text` holds the signal's address; `size`, `lh` its size).
pub(crate) const K_INPUT: u8 = 5;
/// A raster image (`text` holds its `ImageBox` in the bump store).
pub(crate) const K_IMAGE: u8 = 6;
/// A producer (`produce(&self.src, &self.reader, request)`): no records; `text` holds its [`ProdReq`].
pub(crate) const K_PROD: u8 = 7;
/// A boundary (`boundary().loading(..).failed(..).child(..)`): three children, the arms, in that order;
/// `text` holds its [`BoundReq`].
pub(crate) const K_BOUND: u8 = 8;

/// The box is painted (`bg`, `rounded`, `border`).
pub(crate) const F_DRAW: u8 = 1;
pub(crate) const F_BORDER: u8 = 2;
/// Text: wraps at its width (a paragraph), else one line.
pub(crate) const F_WRAP: u8 = 4;
/// Text: the content is the integer in `len` (formatted when it is needed), not bytes at `text`.
pub(crate) const F_INT: u8 = 8;
/// Text: that integer is negative (`len` holds its two's complement).
pub(crate) const F_NEG: u8 = 16;

/// Reactive operations present in a box's [`Ext`] (`boxops.rs`).
pub(crate) const OP_ALPHA: u8 = 1;
pub(crate) const OP_TX: u8 = 2;
pub(crate) const OP_TY: u8 = 4;
pub(crate) const OP_W: u8 = 8;
pub(crate) const OP_H: u8 = 16;

/// `Ext::flags`.
pub(crate) const X_HOVER: u8 = 1;
pub(crate) const X_FOCUS: u8 = 2;

/// Route kinds (`ActReq::kind`).
pub(crate) const A_CLICK: u8 = 0;
pub(crate) const A_PRESS: u8 = 1;
pub(crate) const A_HOVER: u8 = 2;
pub(crate) const A_DRAG: u8 = 3;
/// A route for one key (`.on_key(key, route)`).
pub(crate) const A_KEY: u8 = 4;
/// Any key (`.on_any_key(route)`).
pub(crate) const A_ANY_KEY: u8 = 5;

/// One described element. Plain data: nothing here has a destructor.
#[repr(C)]
pub(crate) struct DNode {
    /// The turn this node was written in; 0 once that turn's arena was rewound.
    pub turn: u64,
    pub next: *mut DNode,
    pub first: *mut DNode,
    pub last: *mut DNode,
    pub ext: *mut Ext,
    /// Box: the background's wire. Text: the ink's.
    pub ink: *const WireReq,
    /// Text: the content's wire.
    pub tw: *const WireReq,
    /// Text: the content's bytes (UTF-8, `len` of them), borrowed for the description's lifetime or copied
    /// into the arena.
    pub text: *const u8,
    /// Text: the byte length, or the integer (`F_INT`).
    pub len: u64,
    pub kind: u8,
    pub flags: u8,
    /// `OP_*` present in `ext`.
    pub ops: u8,
    /// `H_*`: which of `ink`, `tw`, `ext` are set (kept by the setters that set them).
    pub has: u8,
    /// Box: background colour. Text: ink.
    pub paint: u32,
    /// Border colour.
    pub bc: u32,
    pub spec: Spec,
    /// Box: corner radius and border width. Text: font size and line height. Text input: its size.
    pub m: [f32; 2],
}

/// `DNode::has`.
pub(crate) const H_INK: u8 = 1;
pub(crate) const H_TW: u8 = 2;
pub(crate) const H_EXT: u8 = 4;

/// The bytes of a node's fixed operands, `kind` through `m`: the start of its seal, compared as one run.
pub(crate) const BLOCK: usize = 104;
const _: () = {
    use std::mem::offset_of;
    assert!(offset_of!(DNode, m) + 8 - offset_of!(DNode, kind) == BLOCK && size_of::<DNode>() == offset_of!(DNode, m) + 8);
    assert!(offset_of!(DNode, kind) % 8 == 0 && offset_of!(DNode, has) == offset_of!(DNode, kind) + 3 && offset_of!(DNode, paint) == offset_of!(DNode, kind) + 4);
    assert!(offset_of!(DNode, spec) == offset_of!(DNode, kind) + 12 && size_of::<Spec>() == 84);
};

impl DNode {
    /// `kind` through `m`: `BLOCK` plain initialised bytes (four `u8`s, two `u32`s, a `Spec` with no padding
    /// (`layout.rs`) and two `f32`s, with no padding between them: the assertions above). Its first word, the
    /// head, is below `1 << 27` (`H_*` < 8).
    #[inline(always)]
    pub(crate) fn block(&self) -> *const u8 {
        &self.kind as *const u8
    }
}

/// What few elements have: reactive operations, routes, hover style. Allocated on first use.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct Ext {
    pub alpha: f32,
    pub tx: f32,
    pub ty: f32,
    pub w: f32,
    pub h: f32,
    pub hover: u32,
    pub wa: *const WireReq,
    pub wtx: *const WireReq,
    pub wty: *const WireReq,
    pub ww: *const WireReq,
    pub wh: *const WireReq,
    /// The element's routes, most recently described first.
    pub acts: *const ActReq,
    pub flags: u8,
}

impl Ext {
    pub(crate) const NEW: Ext = Ext { alpha: 1.0, tx: 0.0, ty: 0.0, w: 0.0, h: 0.0, hover: 0, wa: ptr::null(), wtx: ptr::null(), wty: ptr::null(), ww: ptr::null(), wh: ptr::null(), acts: ptr::null(), flags: 0 };
}

/// A port asked to be wired: the cell it reads, and the patch that writes the node from it. The walk
/// installs it when the cell is a field of the mounting owner (`Mounter::wire_field`).
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct WireReq {
    /// The head the wire hangs on (the cell's own, its first field, or a reader's pending head), its two low
    /// bits (heads are 4-aligned) tagged with the wire's kind (`W_*`), so a wire's identity (seal, shape) is this
    /// word, the patch and the recipe as before.
    pub head: *const Cell<u32>,
    /// `reflect::cell_id` of the cell type `patch` reads.
    pub ty: TypeId,
    pub patch: PatchFn,
    pub recipe: crate::rt::Recipe,
}

/// `WireReq::head` tags: a `latest(..)` port (it runs while a cohort holds its signal).
pub(crate) const W_LATEST: usize = 1;
/// A reader's pending head (`phead`, 4 bytes into the cell): `is_pending(..)` ports.
pub(crate) const W_PHEAD: usize = 2;
/// A reader's value head, read by a port: a boundary around it guards that reader (`Boundary::child`).
pub(crate) const W_GUARD: usize = 3;

/// `h` tagged `t` (`W_*`).
#[inline(always)]
pub(crate) fn tagged(h: &Cell<u32>, t: usize) -> *const Cell<u32> {
    (h as *const Cell<u32> as usize | t) as *const Cell<u32>
}

impl WireReq {
    /// The wire's kind (`W_*`, 0: a plain port).
    #[inline(always)]
    pub(crate) fn tag(&self) -> usize {
        self.head as usize & 3
    }
    /// The head the wire hangs on.
    #[inline(always)]
    pub(crate) fn wire_head(&self) -> *const Cell<u32> {
        (self.head as usize & !3) as *const Cell<u32>
    }
    /// Bytes from the cell's start to the head the wire hangs on.
    #[inline(always)]
    pub(crate) fn sub(&self) -> usize {
        if self.tag() == W_PHEAD { 4 } else { 0 }
    }
    /// The cell's address.
    #[inline(always)]
    pub(crate) fn cell(&self) -> usize {
        self.wire_head() as usize - self.sub()
    }
}

/// A producer as described: its source signal and its reader (proven fields of the view at mount), the
/// request function, and the kernel's mount for this source and reader type.
#[derive(Clone, Copy)]
pub(crate) struct ProdReq {
    pub src: *const u8,
    pub src_ty: TypeId,
    pub dst: *const u8,
    pub dst_ty: TypeId,
    /// The `fn(&S) -> Request<T>`.
    pub f: usize,
    pub mount: crate::reader::MountProducer,
    /// (source type, reader type) names, for a located refusal.
    pub names: fn() -> (&'static str, &'static str),
    pub at: &'static std::panic::Location<'static>,
}

/// A reader a boundary guards: one of the readers its child arm reads (`W_GUARD` ports), once each.
#[derive(Clone, Copy)]
pub(crate) struct GuardReq {
    pub next: *const GuardReq,
    /// The reader's address.
    pub reader: usize,
    pub ty: TypeId,
}

/// A boundary as described: its guards and its failure policy.
#[derive(Clone, Copy)]
pub(crate) struct BoundReq {
    pub guards: *const GuardReq,
    /// `crate::reader::BP_*`.
    pub policy: u8,
}

/// A route asked for: the trampoline monomorphised where the route was written, the route's bytes, and the
/// receiver type the trampoline reads its owner as.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct ActReq {
    pub next: *const ActReq,
    pub tramp: RouteFn,
    pub recipe: crate::rt::Recipe,
    pub recv: TypeId,
    pub recv_name: &'static str,
    pub key: Key,
    pub kind: u8,
}

const CHUNK: usize = 256;
/// Bump chunk size, in 8-byte words.
const AUX_WORDS: usize = 4096;

struct Arena {
    /// Node chunks, `CHUNK` nodes each. Never freed (see the module docs).
    chunks: ManuallyDrop<Vec<NonNull<MaybeUninit<DNode>>>>,
    /// The next free node slot and the end of its chunk (null, null: no chunk yet).
    cur: *mut DNode,
    end: *mut DNode,
    /// The chunk `cur` is in (`chunks.len()` before the first).
    at: usize,
    /// Bump chunks (start, length in words). Never freed.
    aux: ManuallyDrop<Vec<(NonNull<u64>, usize)>>,
    /// The bump position: chunk index and words used in it.
    aux_chunk: usize,
    aux_at: usize,
    /// The current turn (never 0).
    turn: u64,
    /// Walks in progress (a walk of a row or an entity view nests in its parent's).
    busy: u32,
}

thread_local! {
    // `ManuallyDrop` fields: the value needs no drop, so no destructor is registered and the arena stays
    // usable (and its nodes readable) until the thread is gone.
    static ARENA: UnsafeCell<Arena> = const {
        UnsafeCell::new(Arena { chunks: ManuallyDrop::new(Vec::new()), cur: ptr::null_mut(), end: ptr::null_mut(), at: 0, aux: ManuallyDrop::new(Vec::new()), aux_chunk: 0, aux_at: 0, turn: 1, busy: 0 })
    };
}

#[inline(always)]
fn with<R>(f: impl FnOnce(&mut Arena) -> R) -> R {
    // SAFETY: the arena is thread-local and `f` is one of this module's leaf functions: none of them calls
    // back into code that could reach the arena again, so this is the only reference for its duration.
    ARENA.with(|a| f(unsafe { &mut *a.get() }))
}

impl Arena {
    #[inline(always)]
    fn node(&mut self, kind: u8, spec: Spec) -> (NonNull<DNode>, u64) {
        if self.cur == self.end {
            self.next_chunk();
        }
        let p = self.cur;
        // SAFETY: `cur < end`, both in one chunk of `CHUNK` slots.
        self.cur = unsafe { p.add(1) };
        let n = DNode {
            turn: self.turn,
            next: ptr::null_mut(),
            first: ptr::null_mut(),
            last: ptr::null_mut(),
            ext: ptr::null_mut(),
            ink: ptr::null(),
            tw: ptr::null(),
            text: ptr::null(),
            len: 0,
            paint: 0,
            bc: 0,
            spec,
            m: [0.0; 2],
            kind,
            flags: 0,
            ops: 0,
            has: 0,
        };
        // SAFETY: `p` is a valid, aligned, unaliased slot (no handle of this turn names it yet).
        unsafe { p.write(n) };
        // SAFETY: `p` is in a chunk, never null.
        (unsafe { NonNull::new_unchecked(p) }, self.turn)
    }
    /// Move to the next chunk, allocating it the first time a turn needs it.
    #[cold]
    #[inline(never)]
    fn next_chunk(&mut self) {
        let next = if self.cur.is_null() { 0 } else { self.at + 1 };
        if next == self.chunks.len() {
            let b: Box<[MaybeUninit<DNode>]> = Box::new_uninit_slice(CHUNK);
            // SAFETY: `Box::into_raw` is never null.
            self.chunks.push(unsafe { NonNull::new_unchecked(Box::into_raw(b) as *mut MaybeUninit<DNode>) });
        }
        self.at = next;
        let c = self.chunks[next].as_ptr() as *mut DNode;
        // SAFETY: the chunk holds `CHUNK` slots.
        (self.cur, self.end) = (c, unsafe { c.add(CHUNK) });
    }
    /// Nodes written this turn.
    fn used(&self) -> usize {
        if self.cur.is_null() {
            return 0;
        }
        // SAFETY: `cur` is in chunk `at`, at or after its start.
        self.at * CHUNK + unsafe { self.cur.offset_from(self.chunks[self.at].as_ptr() as *mut DNode) } as usize
    }
    /// `words` zero-or-more 8-byte words of bump space, 8-aligned.
    #[inline]
    fn words(&mut self, words: usize) -> *mut u64 {
        loop {
            if let Some(&(p, cap)) = self.aux.get(self.aux_chunk) {
                if self.aux_at + words <= cap {
                    // SAFETY: `aux_at + words <= cap`, the chunk's length.
                    let at = unsafe { p.as_ptr().add(self.aux_at) };
                    self.aux_at += words;
                    return at;
                }
                // (a chunk too small for this request is skipped for the rest of the turn)
                self.aux_chunk += 1;
                self.aux_at = 0;
            } else {
                let cap = words.max(AUX_WORDS);
                let b: Box<[MaybeUninit<u64>]> = Box::new_uninit_slice(cap);
                // SAFETY: `Box::into_raw` is never null.
                self.aux.push((unsafe { NonNull::new_unchecked(Box::into_raw(b) as *mut u64) }, cap));
            }
        }
    }
    fn rewind(&mut self) {
        if !self.cur.is_null() {
            // the full chunks before `at`, then chunk `at` up to `cur`
            for (i, c) in self.chunks[..=self.at].iter().enumerate() {
                let mut p = c.as_ptr() as *mut DNode;
                // SAFETY: chunk `i` holds `CHUNK` slots; `cur` is in chunk `at`.
                let end = if i == self.at { self.cur } else { unsafe { p.add(CHUNK) } };
                while p != end {
                    // SAFETY: the slots before `end` were written this turn (`node`); only the turn number is
                    // touched, and no reference to a node is live outside a walk or a setter (both have returned).
                    unsafe {
                        (*p).turn = 0;
                        p = p.add(1);
                    }
                }
            }
        }
        (self.cur, self.end, self.at) = (ptr::null_mut(), ptr::null_mut(), 0);
        self.aux_chunk = 0;
        self.aux_at = 0;
        self.turn += 1;
    }
}

/// A new node of this turn, and the turn number its handle keeps.
#[inline]
pub(crate) fn node(kind: u8, spec: Spec) -> (NonNull<DNode>, u64) {
    with(|a| a.node(kind, spec))
}

/// `v` moved into this turn's bump store. `T` is plain data (it is never dropped).
#[inline]
pub(crate) fn put<T: Copy>(v: T) -> *mut T {
    const { assert!(align_of::<T>() <= 8) };
    let p = with(|a| a.words(size_of::<T>().div_ceil(8))) as *mut T;
    // SAFETY: `p` is 8-aligned (enough for `T`, checked above) and has room for a `T`.
    unsafe { p.write(v) };
    p
}

/// `b` copied into this turn's bump store.
#[inline]
pub(crate) fn bytes(b: &[u8]) -> *const u8 {
    if b.is_empty() {
        return NonNull::<u8>::dangling().as_ptr();
    }
    let p = with(|a| a.words(b.len().div_ceil(8))) as *mut u8;
    // SAFETY: `p` has room for `b.len()` bytes and cannot overlap a borrowed slice.
    unsafe { ptr::copy_nonoverlapping(b.as_ptr(), p, b.len()) };
    p
}

/// A walk of a description (`walk.rs`): opened by the kernel where it describes (a mount, a re-description, a
/// list row, a lowering). While any is alive the arena is not rewound; when the outermost one ends, also by
/// unwinding, this turn's nodes are retired.
pub(crate) struct Walk(bool);

impl Walk {
    /// A kernel boundary: the description it encloses is retired when the outermost boundary ends.
    #[inline]
    pub(crate) fn enter() -> Walk {
        with(|a| a.busy += 1);
        Walk(true)
    }
    /// An `El` walked by itself (an element of a typed builder): its siblings may be `El`s still to be walked,
    /// so it keeps the arena from being rewound but never rewinds it; the next kernel boundary does.
    #[inline]
    pub(crate) fn hold() -> Walk {
        with(|a| a.busy += 1);
        Walk(false)
    }
}

impl Drop for Walk {
    #[inline]
    fn drop(&mut self) {
        let boundary = self.0;
        with(|a| {
            a.busy -= 1;
            if a.busy == 0 && boundary {
                a.rewind();
            }
        });
    }
}

/// (nodes written this turn, bump chunks, node chunks): for the tests that hold the arena to "a steady turn
/// allocates nothing".
#[doc(hidden)]
pub fn stats() -> (usize, usize, usize) {
    with(|a| (a.used(), a.aux.len(), a.chunks.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rewind_retires_the_turns_nodes_and_reuses_their_memory() {
        let spec = Spec::new(true, true);
        let (a, ga) = node(K_BOX, spec);
        let t = bytes(b"hello");
        // SAFETY: `a` is this turn's node; `t` holds the five bytes just copied.
        unsafe {
            assert_eq!((*a.as_ptr()).turn, ga);
            assert_eq!(std::slice::from_raw_parts(t, 5), b"hello");
        }
        drop(Walk::enter());
        // SAFETY: node memory is never freed; the rewind zeroed the turn number.
        assert_eq!(unsafe { (*a.as_ptr()).turn }, 0);
        let (b, gb) = node(K_TEXT, spec);
        assert_eq!(a, b, "the next turn writes the same slot");
        assert_ne!(ga, gb, "under a new turn number");
        let chunks = stats();
        drop(Walk::enter());
        for _ in 0..3 {
            for _ in 0..1000 {
                let _ = node(K_BOX, spec);
                let _ = put(7u64);
            }
            drop(Walk::enter());
        }
        let grown = stats();
        for _ in 0..1000 {
            let _ = node(K_BOX, spec);
            let _ = put(7u64);
        }
        drop(Walk::enter());
        assert!(grown.2 >= chunks.2);
        assert_eq!((grown.1, grown.2), (stats().1, stats().2), "a turn of the same size takes no new chunk");
    }

    #[test]
    fn a_nested_walk_does_not_rewind() {
        let spec = Spec::new(true, true);
        let outer = Walk::enter();
        let (a, ga) = node(K_BOX, spec);
        drop(Walk::enter());
        // SAFETY: `a` is this turn's node.
        assert_eq!(unsafe { (*a.as_ptr()).turn }, ga, "the outer walk still reads its nodes");
        drop(outer);
        // SAFETY: node memory is never freed.
        assert_eq!(unsafe { (*a.as_ptr()).turn }, 0);
    }

    #[test]
    fn an_oversize_request_gets_its_own_chunk() {
        let big = vec![7u8; AUX_WORDS * 8 * 3];
        let p = bytes(&big);
        // SAFETY: `p` holds `big.len()` bytes just copied.
        assert_eq!(unsafe { std::slice::from_raw_parts(p, big.len()) }, &big[..]);
        drop(Walk::enter());
    }
}
