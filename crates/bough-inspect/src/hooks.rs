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

/// Convert a live `bough_x::damage_engine::DamageReport` into the inspector's `DamageMap`
/// (with real damaged tile rects, repair %, opaque-occluded skips, backdrop-blur dilation count,
/// tile bitmap words, per-tile heat, and scroll blit vector).
pub fn damage_map_from_report(report: &bough_x::damage_engine::DamageReport) -> DamageMap {
    let tile = report.tile.max(1);
    let w = report.width.max(0);
    let h = report.height.max(0);
    let mut rects = Vec::with_capacity(report.tiles.len());
    let mut heat = Vec::with_capacity(report.tiles.len());
    let mut damaged_pixels = 0u64;

    let tile_cols = (w as usize).div_ceil(tile as usize);
    let tile_rows = (h as usize).div_ceil(tile as usize);
    let words_per_row = tile_cols.div_ceil(64);
    let mut bitmap = vec![0u64; tile_rows.saturating_mul(words_per_row)];

    for &(tx, ty) in &report.tiles {
        let x0 = (tx as i32).saturating_mul(tile);
        let y0 = (ty as i32).saturating_mul(tile);
        let x1 = (x0 + tile).min(w);
        let y1 = (y0 + tile).min(h);
        if x1 > x0 && y1 > y0 {
            let r = crate::schema::R { x0, y0, x1, y1 };
            rects.push(r);
            damaged_pixels += ((x1 - x0) as u64).saturating_mul((y1 - y0) as u64);
        }
        heat.push(TileHeat {
            x: tx as i32,
            y: ty as i32,
            repaints: 1,
        });
        let (ux, uy) = (tx as usize, ty as usize);
        if uy < tile_rows && (ux / 64) < words_per_row {
            bitmap[uy * words_per_row + (ux / 64)] |= 1u64 << (ux & 63);
        }
    }

    let tile_bitmap_words = bitmap.iter().map(|word| format!("{word:016x}")).collect();
    let viewport_pixels = (w as u64).saturating_mul(h as u64);
    let scroll_blit = report.scroll_blits.first().map(|b| {
        let kept = b.kept();
        Blit {
            col: 0,
            until: 0,
            rect: crate::schema::R {
                x0: kept.x0,
                y0: kept.y0,
                x1: kept.x1,
                y1: kept.y1,
            },
            origin: (b.dx, b.dy),
        }
    });
    let backdrop_dilated_tiles = report.work.blur_tiles.max(report.blur_blocks.len() as u32);

    DamageMap {
        rects,
        damaged_tiles: report.dirty_tiles,
        damaged_pixels,
        viewport_pixels,
        repair_pct: report.repair_percent as f64,
        tile_bitmap_words,
        heat,
        scroll_blit,
        occluded_tiles: report.occluded_tiles_skipped,
        backdrop_dilated_tiles,
        heatmap_png_base64: None,
    }
}

/// Convert a live `bough_x::damage_engine::DamageReport` and measured phase timings into a `FrameProfile`.
pub fn frame_profile_from_report(
    report: &bough_x::damage_engine::DamageReport,
    input_us: u64,
    eval_us: u64,
    layout_us: u64,
    damage_us: u64,
    paint_us: u64,
    submit_us: u64,
    allocated_bytes_warm: u64,
) -> FrameProfile {
    let executor = match report.choice {
        bough_x::damage_engine::ExecutorChoice::Idle => "idle",
        bough_x::damage_engine::ExecutorChoice::Cpu => "cpu",
        bough_x::damage_engine::ExecutorChoice::Gpu => "metal",
    };
    let total_us = input_us
        .saturating_add(eval_us)
        .saturating_add(layout_us)
        .saturating_add(damage_us)
        .saturating_add(paint_us)
        .saturating_add(submit_us);
    let phases = vec![
        Phase { name: "input", us: input_us },
        Phase { name: "eval", us: eval_us },
        Phase { name: "layout", us: layout_us },
        Phase { name: "damage", us: damage_us },
        Phase { name: "paint", us: paint_us },
        Phase { name: "submit", us: submit_us },
    ];
    FrameProfile {
        input_us,
        eval_us,
        layout_us,
        damage_us,
        paint_us,
        submit_us,
        total_us,
        allocated_bytes_warm,
        dirty_nodes: report.work.affected_records as usize,
        dirty_tiles: report.dirty_tiles,
        executor,
        phases,
    }
}

#[cfg(test)]
mod bridge_tests {
    use super::*;
    use bough_x::damage_engine::{BlurLayer, DamageEngine, OpaqueCover, Rect, Work};

    #[test]
    fn damage_engine_report_bridges_to_inspector_protocol_and_heatmap() {
        let mut eng = DamageEngine::new(640, 480);
        eng.begin_frame();
        eng.covers_clear();
        // Opaque modal covering [64..256, 64..256] at z=20
        eng.cover_push(OpaqueCover {
            z: 20,
            core_rect: Rect::new(64, 64, 256, 256),
        });
        eng.covers_commit();
        eng.layers_clear();
        // Backdrop blur glass panel at [320..480, 64..192] at z=15
        eng.layer_push(BlurLayer {
            id: 1,
            z: 15,
            rect: Rect::new(320, 64, 480, 192),
            radius: 16,
            max_level: 3,
        });
        eng.layers_commit();
        // Damage behind opaque modal (occluded!)
        eng.damage(5, Rect::new(96, 96, 160, 160), Work::Flat);
        // Damage behind glass panel (triggers blur propagation!)
        eng.damage(5, Rect::new(352, 96, 384, 128), Work::Flat);
        // Rigid scroll in lower strip
        let _ = eng.scroll(1, 10, Rect::new(0, 288, 640, 480), 0, -16);
        eng.resolve(&mut |_layer, rect| Some(rect));

        let rep = eng.report();
        let dm = damage_map_from_report(&rep);
        assert!(dm.damaged_tiles > 0);
        assert!(dm.occluded_tiles > 0, "expected opaque cover culling to skip occluded tiles");
        assert!(dm.backdrop_dilated_tiles > 0, "expected backdrop blur dilation");
        assert!(dm.scroll_blit.is_some(), "expected rigid scroll blit vector");
        assert!(dm.repair_pct > 0.0 && dm.repair_pct < 60.0);

        let prof = frame_profile_from_report(&rep, 10, 20, 0, 30, 80, 15, 0);
        assert_eq!(prof.total_us, 155);
        assert_eq!(prof.dirty_tiles, rep.dirty_tiles);
        assert_eq!(prof.allocated_bytes_warm, 0);
        let mut encoded = String::new(); crate::json::write(&dm.json(), &mut encoded); assert!(encoded.contains("occluded_tiles")); assert!(encoded.contains("backdrop_dilated_tiles"));
    }
}
