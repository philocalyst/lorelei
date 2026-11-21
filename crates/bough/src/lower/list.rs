//! `list(sig!(self.items))` on the lowered runtime: rows by exposure, identity by slot.
//!
//! A row is an owner of its own (`T: View`): its handlers take `&mut T`, reached at dispatch through the list's
//! proven offset in the parent owner and the row's slot (index and generation), never through a position, so a
//! handler of a row that was removed or replaced since the description finds nothing and does not run.
//!
//! A virtual list (`.viewport()`, `.scroll(..)`) describes only the rows that meet its port, between two
//! spacers that stand for the rest. Where a row is comes from its measured height when it has been described
//! before (kept by slot, so an insert or a reorder moves no measurement) and the estimate otherwise; positions
//! are 64-bit. A description that measured a row at another height than it assumed is run again (bounded), so
//! what is shown never depends on a stale estimate.
//!
//! Stated cost: finding the first exposed row walks the order (one add per row before it), which is O(rows) per
//! description in arithmetic, not in records. The maintained order lane (the incremental-operators lane) is
//! what removes it; the author's code does not change when it does.
use super::{Core, Lower, Owner, Scroll};
use crate::color::Color;
use crate::layout::{Dim, Spec};
use crate::list::List;
use std::any::{Any, TypeId};
use std::collections::HashMap;

pub(crate) type ListKey = (TypeId, u32, u64);

/// What a list keeps between descriptions: measured row heights by slot index (device pixels; 0: not measured).
#[derive(Default)]
pub struct ListState {
    pub(crate) h: Vec<i32>,
}
pub(crate) type Lists = HashMap<ListKey, ListState>;

/// A row a description materialised: which list, which slot, and its wrapper node (document, node).
pub(crate) struct RowRec {
    pub key: ListKey,
    pub slot: u32,
    pub doc: u32,
    pub node: u32,
}

#[doc(hidden)]
pub struct ListParams {
    pub spec: Spec,
    pub bg: Option<Color>,
    pub radius: f32,
    pub virt: bool,
    /// The scroll offset now (points) and the offset of its `Signal<f32>` in the owner.
    pub scroll: Option<(f32, u32)>,
    pub estimate: f32,
    pub overscan: usize,
    pub source: &'static str,
}

fn mix(a: u64, b: u64) -> u64 {
    (a ^ b.wrapping_mul(0x9e37_79b9_7f4a_7c15))
        .rotate_left(27)
        .wrapping_mul(0x100_0000_01b3)
}

/// One row: a wrapper box (what is measured), under an owner that reaches the item by its slot.
fn row<V: 'static, T: 'static>(
    core: &mut Core<'_>,
    key: ListKey,
    list: &List<T>,
    off: u32,
    pos: usize,
    f: for<'c, 'a> fn(&T, &mut Lower<'c, 'a, T>),
) {
    let slot = list.slot(pos).expect("a position under the list's length");
    let item = list.get(pos).expect("a position under the list's length");
    let (parent, parent_key) = (core.owner, core.owner_key);
    core.owners.push(Owner {
        parent,
        project: Box::new(move |o: &mut dyn Any| {
            let v = o.downcast_mut::<V>()?;
            // SAFETY: `off` is the offset of an `LWired<'_, T, V>`, which only `sig!` makes: `offset_of!(V, path)`
            // of a struct field whose type the macro pins to `List<T>` (`signal.rs`, "Soundness of wires"). `v`
            // is an exclusive borrow of that `V`: the field is in bounds, initialised and not otherwise borrowed.
            let l = unsafe { &mut *(v as *mut V as *mut u8).add(off as usize).cast::<List<T>>() };
            // by slot and generation: an item taken out since the description is not found
            l.edit_slot(slot).map(|t| t as &mut dyn Any)
        }),
    });
    core.owner = core.owners.len() as u32;
    core.owner_key = mix(
        mix(parent_key, off as u64),
        (slot.index() as u64) << 32 | slot.generation as u64,
    );
    let w = core.open(&Spec::new(true, true), "list row");
    core.rows.push(RowRec {
        key,
        slot: slot.index(),
        doc: core.cur as u32,
        node: w as u32,
    });
    f(item, &mut Lower::<T>::new(core));
    core.close(w);
    (core.owner, core.owner_key) = (parent, parent_key);
}

pub(crate) fn lower_list<V: 'static, T: 'static>(
    l: &mut Lower<'_, '_, V>,
    p: ListParams,
    list: &List<T>,
    off: u32,
    f: for<'c, 'a> fn(&T, &mut Lower<'c, 'a, T>),
) {
    let core = &mut *l.core;
    let key: ListKey = (TypeId::of::<V>(), off, core.owner_key);
    let s = core.scale;
    let n = list.len();
    let paint = |core: &mut Core<'_>, i: usize| {
        let node = core.node(i);
        if let Some(c) = p.bg {
            node.paint = super::Paint::Box {
                color: c.0,
                radius: p.radius,
                border: None,
            };
            node.solid = true;
        }
    };
    if !p.virt {
        // every row is a child of the container, in flow
        let i = core.open(&p.spec, p.source);
        paint(core, i);
        for pos in 0..n {
            row::<V, T>(core, key, list, off, pos, f);
        }
        core.close(i);
        return;
    }
    // the port: a box sized by its parent's layout; the rows are another document, cut to it
    let mut port = p.spec;
    (port.container, port.pad, port.gap) = (false, [0.0; 4], 0.0);
    let i = core.open(&port, p.source);
    paint(core, i);
    core.node(i).solid = true;
    core.close(i);
    let port_h = match p.spec.h.parts() {
        (1, v) => (v * s).round() as i64,
        (3, v) => v.round() as i64,
        // not a fixed height: the window bounds what can be exposed
        _ => core.win.1 as i64,
    };
    let gap = (p.spec.gap * s).round() as i64;
    let est = ((p.estimate.max(1.0)) * s).round() as i64;
    let at = p.scroll.map_or(0.0, |x| x.0);
    let top = (at * s).round() as i64 - (p.spec.pad[1] * s).round() as i64;
    let order = list.order_slots();
    // (the measured heights are looked up once, not per row)
    let lists = core.lists;
    let measured: &[i32] = lists.get(&key).map_or(&[], |st| &st.h[..]);
    let height = |_: &Core<'_>, pos: usize| -> i64 {
        match measured.get(order[pos] as usize) {
            Some(&h) if h > 0 => h as i64,
            _ => est,
        }
    };
    // the first row whose bottom is past the port's top, the last whose top is before its bottom
    let (mut y, mut lo) = (0i64, 0usize);
    while lo < n {
        let h = height(core, lo);
        if y + h > top {
            break;
        }
        y += h + gap;
        lo += 1;
    }
    let (mut yb, mut hi) = (y, lo);
    while hi < n && yb < top + port_h {
        yb += height(core, hi) + gap;
        hi += 1;
    }
    // overscan: rows beyond each edge
    for _ in 0..p.overscan {
        if lo > 0 {
            lo -= 1;
            y -= height(core, lo) + gap;
        }
        if hi < n {
            hi += 1;
        }
    }
    let mut rest = 0i64;
    for pos in hi..n {
        rest += height(core, pos) + gap;
    }
    let owner = core.owner;
    let doc = core.sub(|core| {
        let mut content = Spec::new(true, true);
        (content.pad, content.gap, content.items) = (p.spec.pad, p.spec.gap, p.spec.items);
        content.w = Dim::Pct(1.0);
        let c = core.open(&content, "list content");
        let spacer = |core: &mut Core<'_>, px: i64| {
            // (a spacer stands for rows and the gaps between them; the gap after it is the flow's)
            let mut sp = Spec::new(false, false);
            sp.shrink = 0.0;
            core.line(&sp, 0.0, (px - gap).max(0) as f32, "list spacer");
        };
        if lo > 0 {
            spacer(core, y);
        }
        for pos in lo..hi {
            row::<V, T>(core, key, list, off, pos, f);
        }
        if hi < n {
            spacer(core, rest);
        }
        core.close(c);
    });
    core.node(i).scroll = Some(Scroll {
        doc,
        at,
        line: 48.0,
        owner,
        set: match p.scroll {
            Some((_, soff)) => Box::new(move |o: &mut dyn Any, v: f32| {
                if let Some(o) = o.downcast_mut::<V>() {
                    // SAFETY: `soff` is the offset of a `Wired<'_, f32, V>` (only `sig!` makes one): a
                    // `Signal<f32>` field of `V`, of which `o` is an exclusive borrow.
                    let sig = unsafe {
                        &mut *(o as *mut V as *mut u8)
                            .add(soff as usize)
                            .cast::<crate::signal::Signal<f32>>()
                    };
                    sig.set(v);
                }
            }),
            None => Box::new(|_, _| {}),
        },
    });
}
