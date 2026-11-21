//! The damage of one commit, captured before a painter consumes it.
use crate::schema::{Damage, R, Sweep};
use bough_x::expr::U;
use bough_x::store::Tiles;

/// Copy the marked tiles and sweeps. The bit rows are copied only for the rows that may hold a mark (`lo..hi`),
/// which is what a painter reads; a full-window mount is one row per tile row.
pub fn capture(t: &Tiles, u: &U) -> Damage {
    let mut rows = Vec::new();
    if t.lo < t.hi {
        for y in t.lo..t.hi {
            rows.push((y, t.bits[y * t.stride..(y + 1) * t.stride].to_vec()));
        }
    }
    let tiles = t.count();
    let mut rects = Vec::new();
    if tiles <= 4096 {
        t.each(|r| rects.push(R::of(r)));
    }
    Damage {
        tiles,
        rows,
        sweeps: t
            .sweeps
            .iter()
            .map(|s| Sweep {
                col: s.col,
                until: s.until,
                rect: R::of(s.rect),
                origin: s.space.origin(u),
            })
            .collect(),
        rects,
    }
}
