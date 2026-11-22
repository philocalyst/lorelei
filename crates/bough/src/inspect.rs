//! What an inspector reads from a running app (round 4, input agent): the public, read-only view of the
//! runtime the inspector app is built on (INSPECTOR.md logs why each accessor exists). Reflection of the
//! owner's state comes from the api agent's typed reflection, not from here.
use crate::app::{App, View};
use bough_core::NRect;

/// One record as the inspector shows it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RecordView {
    pub pos: usize,
    pub depth: u16,
    /// The kind byte (low bits: quad / text / box; high bits: interactive, focusable, pointer-transparent).
    pub kind: u8,
    /// Absolute ink (device px; a transformed record's: its transformed bounding box).
    pub ink: NRect,
    pub handle: u32,
    pub clip: u16,
    /// The layout box (absolute device px: x, y, w, h), for records that are layout nodes.
    pub layout: Option<[f32; 4]>,
    /// Whether the record is in a transformed subtree.
    pub transformed: bool,
}

impl RecordView {
    pub fn kind_name(&self) -> &'static str {
        match self.kind & 0x1F {
            crate::rt::REC_QUAD => "quad",
            crate::rt::REC_TEXT => "text",
            crate::rt::REC_BOX => "box",
            _ => "record",
        }
    }
}

impl<V: View> App<V> {
    /// Records in the tree (preorder).
    pub fn record_count(&self) -> usize {
        self.rt.bough.len()
    }

    /// Record `pos` with its layout box.
    pub fn record_view(&self, pos: usize) -> RecordView {
        let r = self.rt.bough.record(pos);
        let layout = (r.handle != 0)
            .then(|| self.rt.lay.info.get(r.handle as usize))
            .flatten()
            .filter(|li| li.live && li.written)
            .map(|li| {
                let o = self.rt.space(li.space);
                [o[0] + li.ax, o[1] + li.ay, li.aw, li.ah]
            });
        RecordView { pos, depth: r.depth, kind: r.kind, ink: r.ink, handle: r.handle, clip: r.clip, layout, transformed: self.rt.bough.xform_at(pos).is_some() }
    }

    /// The topmost pickable record at window point (x, y) (points), as hit testing answers it.
    pub fn pick_record(&self, x: f32, y: f32) -> Option<usize> {
        let s = self.rt.scale;
        self.rt.bough.hit_top((x * s) as i32, (y * s) as i32)
    }

    /// The ancestors of `pos` (nearest first).
    pub fn record_ancestors(&self, pos: usize) -> Vec<usize> {
        let mut v = Vec::new();
        self.rt.bough.ancestors(pos, &mut v);
        v
    }

    /// The tiles the last drawn frame repaired: per 32-px tile row, a bitmask of tile columns (every tile of
    /// the window when the frame was a full repaint).
    pub fn last_repair(&self) -> Vec<u128> {
        let (rows, full) = self.repaired_rows();
        if !full {
            return rows.to_vec();
        }
        let tx = (self.rt.win_w as usize).div_ceil(32);
        let all = if tx >= 128 { u128::MAX } else { (1u128 << tx) - 1 };
        vec![all; (self.rt.win_h as usize).div_ceil(32)]
    }
}

/// Live damage summary for the last drawn frame, computed on demand when the inspector queries it
/// (zero hot-path overhead when inspection is off).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DamageSummary {
    pub viewport_w: i32,
    pub viewport_h: i32,
    pub full_repaint: bool,
    pub dirty_tiles: u32,
    pub total_tiles: u32,
    pub repair_pct: f64,
}

/// Live per-frame execution profile read from the runtime & kernel backend on demand.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameProfileView {
    pub frames: u64,
    pub records: usize,
    pub dirty_tiles: u32,
    pub staged_cmds: u64,
    pub bin_entries: u64,
    pub est_cpu_ns: u64,
    pub executor: &'static str,
}

impl<V: View> App<V> {
    /// Summarize the last frame's 32x32 tile repair region (dirty tiles, total tiles, repair %).
    pub fn last_damage_summary(&self) -> DamageSummary {
        let tx = (self.rt.win_w as u32).div_ceil(32);
        let ty = (self.rt.win_h as u32).div_ceil(32);
        let total_tiles = tx.saturating_mul(ty);
        let (rows, full) = self.repaired_rows();
        let dirty_tiles = if full {
            total_tiles
        } else {
            let mask = if tx >= 128 { u128::MAX } else { (1u128 << tx) - 1 };
            rows.iter()
                .take(ty as usize)
                .map(|r| (r & mask).count_ones())
                .sum()
        };
        let repair_pct = if total_tiles == 0 {
            0.0
        } else {
            (dirty_tiles as f64 / total_tiles as f64) * 100.0
        };
        DamageSummary {
            viewport_w: self.rt.win_w,
            viewport_h: self.rt.win_h,
            full_repaint: full,
            dirty_tiles,
            total_tiles,
            repair_pct,
        }
    }

    /// Snapshot the last frame's execution profile (zero-allocation, read on demand).
    pub fn last_frame_profile(&self) -> FrameProfileView {
        let dmg = self.last_damage_summary();
        let (staged_cmds, bin_entries, est_cpu_ns, executor) = match &self.kernel {
            Some(k) => {
                let exec = match k.last_frame.exec_kind {
                    0 => "cpu1",
                    1 => "cpun",
                    _ => "metal",
                };
                (k.last_frame.staged, k.last_frame.entries, k.last_frame.est_cpu_ns, exec)
            }
            None => (0, 0, 0, "presenter"),
        };
        FrameProfileView {
            frames: self.frames,
            records: self.rt.bough.len(),
            dirty_tiles: dmg.dirty_tiles,
            staged_cmds,
            bin_entries,
            est_cpu_ns,
            executor,
        }
    }
}
