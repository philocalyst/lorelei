//! What a frame's damage was, for tools.
use crate::{BlurBlock, ExecutorChoice, ScrollBlit, WorkEstimate};

/// A frame's damage as data, for an inspector or the `damage_map` tool.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DamageReport {
    pub width: i32,
    pub height: i32,
    pub tile: i32,
    /// Dirty tiles as (column, row).
    pub tiles: Vec<(u16, u16)>,
    pub dirty_tiles: u32,
    pub total_tiles: u32,
    pub repair_percent: f32,
    /// Distinct tiles some damage wanted and no damage got: hidden under an opaque cover.
    pub occluded_tiles_skipped: u32,
    /// (damage, tile) pairs skipped.
    pub occlusion_skips: u32,
    pub blur_blocks: Vec<BlurBlock>,
    /// Blur propagations a gate halted (the backdrop repainted to the same pixels).
    pub blur_gated: u32,
    pub scroll_blits: Vec<ScrollBlit>,
    pub covers: u32,
    pub layers: u32,
    pub work: WorkEstimate,
    pub cost: u64,
    pub choice: ExecutorChoice,
    pub reason: &'static str,
}

impl DamageReport {
    /// The report as one line of JSON (no dependency: the fields are numbers and fixed strings).
    pub fn to_json(&self) -> String {
        use std::fmt::Write;
        let mut s = String::new();
        let _ = write!(
            s,
            "{{\"width\":{},\"height\":{},\"tile\":{},\"dirty_tiles\":{},\"total_tiles\":{},\"repair_percent\":{:.3},\"occluded_tiles_skipped\":{},\"occlusion_skips\":{},\"blur_gated\":{},\"covers\":{},\"layers\":{},\"fanout\":{},\"affected_records\":{},\"shadow_tiles\":{},\"grad_tiles\":{},\"blur_tiles\":{},\"cost\":{},\"choice\":\"{:?}\",\"reason\":\"{}\",\"tiles\":[",
            self.width, self.height, self.tile, self.dirty_tiles, self.total_tiles, self.repair_percent,
            self.occluded_tiles_skipped, self.occlusion_skips, self.blur_gated, self.covers, self.layers,
            self.work.fanout, self.work.affected_records, self.work.shadow_tiles, self.work.grad_tiles,
            self.work.blur_tiles, self.cost, self.choice, self.reason
        );
        for (i, (x, y)) in self.tiles.iter().enumerate() {
            let _ = write!(s, "{}[{x},{y}]", if i == 0 { "" } else { "," });
        }
        s.push_str("],\"blur_blocks\":[");
        for (i, b) in self.blur_blocks.iter().enumerate() {
            let _ = write!(s, "{}{{\"layer\":{},\"level\":{},\"x0\":{},\"y0\":{},\"x1\":{},\"y1\":{}}}", if i == 0 { "" } else { "," }, b.layer, b.level, b.x0, b.y0, b.x1, b.y1);
        }
        s.push_str("],\"scroll_blits\":[");
        for (i, b) in self.scroll_blits.iter().enumerate() {
            let c = b.clip_rect;
            let _ = write!(s, "{}{{\"clip\":[{},{},{},{}],\"dx\":{},\"dy\":{}}}", if i == 0 { "" } else { "," }, c.x0, c.y0, c.x1, c.y1, b.dx, b.dy);
        }
        s.push_str("]}");
        s
    }
}
