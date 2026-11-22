//! Deltas that visit the records a change concerns, for columns whose archetype declares what bounds its
//! change (the incremental-operators card). Two declarations, both made by the expression's type:
//!
//! * a **key** ([`Arch::KEYED`], from `expr::Sel`): when only the selecting word of the block changed, a record
//!   can show something else only if its key is the old or the new value of that word. With the column's
//!   records sorted by key ([`key_build`]), the delta is two searches and the records they find
//!   ([`key_delta`]): two rows for "which row is selected", whatever is exposed.
//! * a **reach** ([`Arch::NEAR`], from `expr::Near`): when only the pointer words changed, a record can show
//!   something else only if its centre is within the reach of the old or the new pointer. The delta tests the
//!   centre words before evaluating anything ([`near_delta`]), and with one box per page ([`near_boxes`]) it
//!   does not open a page the pointer is not near.
//!
//! Neither structure is maintained by the write path. The world builds them when a column takes the fast path
//! for the second time running and drops them whenever a description pass runs, which is the only time a
//! record's operands, its liveness or a page can change (`World::delta_one`). Both kernels evaluate the same
//! `changed(old, new)` the page scan evaluates, on a subset proved to contain every record it can be true
//! for, so the damage they leave is the scan's, bit for bit (`tests/select.rs`).
use super::{Arch, ColEnt, Page, Tiles, U, mark_page};
use crate::expr::{U_PTR_X, U_PTR_Y, within};

/// The records of a column by key: `(key word, record index)`, sorted.
pub type KeyBuild = unsafe fn(&ColEnt, &mut Vec<(u32, u32)>);
/// Damage from a change of only the key's word. Returns the records evaluated.
pub type KeyDelta = unsafe fn(&ColEnt, &[(u32, u32)], &U, &U, &mut Tiles) -> u32;
/// One box per page: the union of its live records' reach, `[x0, y0, x1, y1]` inclusive.
pub type NearBoxes = unsafe fn(&ColEnt, &mut Vec<[i32; 4]>);
/// Damage from a change of only the pointer words. Returns the records evaluated.
pub type NearDelta = unsafe fn(&ColEnt, Option<&[[i32; 4]]>, &U, &U, &mut Tiles) -> u32;

/// What an archetype declares, as the functions that use it. `None` twice for nearly every archetype.
#[derive(Clone, Copy)]
pub struct FastFns {
    /// The selecting word of the block, and the kernels.
    pub key: Option<(u8, KeyBuild, KeyDelta)>,
    pub near: Option<(NearBoxes, NearDelta)>,
}
pub const fn fns<A: Arch>() -> FastFns {
    FastFns {
        key: match A::KEYED {
            // only a record that is whole words has a word to read a key from
            Some((word, _)) if A::WORD_LAYOUT => Some((word as u8, key_build::<A>, key_delta::<A>)),
            _ => None,
        },
        near: match A::NEAR {
            Some(_) if A::WORD_LAYOUT => Some((near_boxes::<A>, near_delta::<A>)),
            _ => None,
        },
    }
}

/// SAFETY (all four): the entry is current and was made for a column of archetype `A` (the caller reached
/// these through that column's own descriptor), and the world holds the stores exclusively.
unsafe fn pages<'a, A: Arch>(e: &'a ColEnt) -> &'a [Page<A>] {
    // SAFETY: the caller's contract: `pages` holds `npages` pages of `A`.
    unsafe { core::slice::from_raw_parts(e.pages as *const Page<A>, e.npages as usize) }
}

unsafe fn key_build<A: Arch>(e: &ColEnt, out: &mut Vec<(u32, u32)>) {
    let Some((_, at)) = A::KEYED else { return };
    out.clear();
    // SAFETY: the caller's contract.
    let (pages, lv) = unsafe { (pages::<A>(e), e.live()) };
    for (pi, pg) in pages.iter().enumerate() {
        let mut live = lv.page(pi);
        while live != 0 {
            let i = live.trailing_zeros() as usize;
            live &= live - 1;
            out.push((pg.record(i).record_words()[at], (pi * 64 + i) as u32));
        }
    }
    out.sort_unstable();
}

unsafe fn key_delta<A: Arch>(
    e: &ColEnt,
    keys: &[(u32, u32)],
    old: &U,
    new: &U,
    tiles: &mut Tiles,
) -> u32 {
    let Some((word, _)) = A::KEYED else { return 0 };
    // SAFETY: the caller's contract.
    let (pages, space) = unsafe { (pages::<A>(e), &*e.space) };
    let (org, clip) = (space.origin(new), space.clip);
    let (ov, nv) = (old.w[word], new.w[word]);
    let mut n = 0u32;
    let mut one = |v: u32| {
        let from = keys.partition_point(|k| k.0 < v);
        for &(k, rec) in &keys[from..] {
            if k != v {
                break;
            }
            n += 1;
            let (pg, i) = (&pages[rec as usize >> 6], rec as usize & 63);
            if pg.record(i).changed(old, new) {
                mark_page(pg, 1 << i, org, clip, old, new, tiles);
                if A::STATE {
                    // SAFETY: the world's delta holds the stores exclusively; record `i` is live and an `A`.
                    unsafe {
                        let p = pg.rec[i].get().cast::<A>();
                        let was = *p;
                        (*p).carry(&was, old, new);
                    }
                }
            }
        }
    };
    #[cfg(not(feature = "mutant_keyed"))]
    one(ov);
    #[cfg(feature = "mutant_keyed")]
    let _ = ov;
    if nv != ov {
        one(nv);
    }
    n
}

/// The reach a record declares, as a box around its centre.
#[inline(always)]
fn reach<A: Arch>(a: &A) -> (i32, i32, i32) {
    let (at, r) = A::NEAR.expect("asked only of an archetype that declares a reach");
    let w = a.record_words();
    #[cfg(feature = "mutant_near")]
    let r = r / 2;
    (w[at] as i32, w[at + 1] as i32, r)
}

unsafe fn near_boxes<A: Arch>(e: &ColEnt, out: &mut Vec<[i32; 4]>) {
    out.clear();
    // SAFETY: the caller's contract.
    let (pages, lv) = unsafe { (pages::<A>(e), e.live()) };
    for (pi, pg) in pages.iter().enumerate() {
        // empty: no point is inside
        let mut b = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
        let mut live = lv.page(pi);
        while live != 0 {
            let i = live.trailing_zeros() as usize;
            live &= live - 1;
            let (x, y, r) = reach(pg.record(i));
            b = [
                b[0].min(x.saturating_sub(r)),
                b[1].min(y.saturating_sub(r)),
                b[2].max(x.saturating_add(r)),
                b[3].max(y.saturating_add(r)),
            ];
        }
        out.push(b);
    }
}

unsafe fn near_delta<A: Arch>(
    e: &ColEnt,
    boxes: Option<&[[i32; 4]]>,
    old: &U,
    new: &U,
    tiles: &mut Tiles,
) -> u32 {
    // SAFETY: the caller's contract.
    let (pages, space, lv) = unsafe { (pages::<A>(e), &*e.space, e.live()) };
    let (org, clip) = (space.origin(new), space.clip);
    let (ox, oy) = (old.w[U_PTR_X] as i32, old.w[U_PTR_Y] as i32);
    let (nx, ny) = (new.w[U_PTR_X] as i32, new.w[U_PTR_Y] as i32);
    let inside =
        |b: &[i32; 4], x: i32, y: i32| (x >= b[0]) & (x <= b[2]) & (y >= b[1]) & (y <= b[3]);
    let mut n = 0u32;
    for (pi, pg) in pages.iter().enumerate() {
        if let Some(b) = boxes
            && !(inside(&b[pi], ox, oy) | inside(&b[pi], nx, ny))
        {
            continue;
        }
        let mut live = lv.page(pi);
        let mut m = 0u64;
        while live != 0 {
            let i = live.trailing_zeros() as usize;
            live &= live - 1;
            let a = pg.record(i);
            let (x, y, r) = reach(a);
            if within(ox, oy, x, y, r) | within(nx, ny, x, y, r) {
                n += 1;
                m |= (a.changed(old, new) as u64) << i;
            }
        }
        if m != 0 {
            mark_page(pg, m, org, clip, old, new, tiles);
            if A::STATE {
                let mut k = m;
                while k != 0 {
                    let i = k.trailing_zeros() as usize;
                    k &= k - 1;
                    // SAFETY: as in `key_delta`.
                    unsafe {
                        let p = pg.rec[i].get().cast::<A>();
                        let was = *p;
                        (*p).carry(&was, old, new);
                    }
                }
            }
        }
    }
    n
}
