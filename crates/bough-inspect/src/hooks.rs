//! Default, snapshot-derived provider implementations, shared by `Program`'s default hook methods.
//!
//! A root integration plugs a live `DamageEngine`, `FlatPaintScene` or `AnimBank` in by overriding the matching
//! `Program` method (see [`crate::session::Program`]); every method's default lives here and costs nothing more
//! than reading the snapshot it already built.
use crate::protocol::{
    Animation, Blit, DamageMap, EffectsInspect, FrameProfile, Phase, ScrubResult, TileHeat,
};
use crate::schema::Snapshot;

/// The damage map the snapshot already knows: rects, the tile bitmap, repair share and per-tile heat.
pub(crate) fn default_damage_map(snap: &Snapshot) -> DamageMap {
    let mut damaged_pixels = 0u64;
    let mut rects = Vec::with_capacity(snap.damage.rects.len());
    for r in &snap.damage.rects {
        rects.push(*r);
        damaged_pixels += (r.w().max(0) as u64).saturating_mul(r.h().max(0) as u64);
    }
    let viewport_pixels = (snap.app.w.max(0) as u64).saturating_mul(snap.app.h.max(0) as u64);
    let repair_pct = if viewport_pixels == 0 {
        0.0
    } else {
        damaged_pixels as f64 * 100.0 / viewport_pixels as f64
    };
    let mut tile_bitmap_words = Vec::new();
    let mut heat = Vec::new();
    for (y, words) in &snap.damage.rows {
        tile_bitmap_words.extend(words.iter().map(|w| format!("{w:016x}")));
        for (wi, w) in words.iter().enumerate() {
            let mut bits = *w;
            let mut bit = 0u32;
            while bits != 0 {
                if bits & 1 != 0 {
                    heat.push(TileHeat {
                        x: (wi * 64 + bit as usize) as i32,
                        y: *y as i32,
                        repaints: 1,
                    });
                }
                bits >>= 1;
                bit += 1;
            }
        }
    }
    let scroll_blit = snap.damage.sweeps.first().map(|s| Blit {
        col: s.col,
        until: s.until,
        rect: s.rect,
        origin: s.origin,
    });
    DamageMap {
        rects,
        damaged_tiles: snap.damage.tiles,
        damaged_pixels,
        viewport_pixels,
        repair_pct,
        tile_bitmap_words,
        heat,
        scroll_blit,
        occluded_tiles: 0,
        backdrop_dilated_tiles: 0,
        heatmap_png_base64: None,
    }
}

/// The default effects inventory: no shadows, blurs, gradients or opacity groups. Only a paint scene knows
/// its own.
pub(crate) fn default_effects_inspect(node: Option<usize>) -> EffectsInspect {
    EffectsInspect {
        node,
        shadows: Vec::new(),
        blurs: Vec::new(),
        gradients: Vec::new(),
        isolated_groups: Vec::new(),
    }
}

/// The default scrub: nothing to scrub, reported honestly.
pub(crate) fn default_scrub_animation() -> ScrubResult {
    ScrubResult {
        ok: false,
        time_ms: 0.0,
        paused: false,
        settled: Vec::new(),
        unsupported: true,
    }
}

/// A snapshot-derived frame profile: the retained bytes, the last settle/paint timings and the moving nodes.
pub(crate) fn default_profile_frame(snap: &Snapshot) -> FrameProfile {
    let settle = snap.counters.last_settle_us;
    let paint = snap.counters.last_paint_us;
    let dirty_nodes = snap.nodes.iter().filter(|n| n.moving).count();
    let phases = vec![
        Phase { name: "input", us: 0 },
        Phase { name: "eval", us: 0 },
        Phase { name: "layout", us: 0 },
        Phase { name: "damage", us: settle },
        Phase { name: "paint", us: paint },
        Phase { name: "submit", us: 0 },
    ];
    FrameProfile {
        input_us: 0,
        eval_us: 0,
        layout_us: 0,
        damage_us: settle,
        paint_us: paint,
        submit_us: 0,
        total_us: settle.saturating_add(paint),
        allocated_bytes_warm: snap.counters.bytes as u64,
        dirty_nodes,
        dirty_tiles: snap.damage.tiles,
        executor: "cpu",
        phases,
    }
}

/// No animations are active by default.
pub(crate) fn default_animations(_snap: &Snapshot) -> Vec<Animation> {
    Vec::new()
}
