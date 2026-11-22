//! Cross-crate integration bridge uniting the 5 Lorelei rethink pillars:
//! - Non-generic SoA core & unified `DamageEngine` (`bough_x::damage_engine`, `bough_x::flat`)
//! - Flat paint bank (`bough_paint::flat_scene::FlatScene`) with full effects (stacked outer/inset
//!   shadows, linear/radial/conic 256-entry LUT gradients, backdrop + element blur, tile-grain groups)
//! - Wrap-safe animation bank (`bough_paint::motion::AnimBank`) and interruptible `PageRouter`
//! - Live telemetry projections ready for `bough-inspect` and `bough-mcp`.
#![forbid(unsafe_code)]

use crate::fx_api::{ClockNs, DrawCmd, FlatScene, Motion, NodeId, RampGeo};
use crate::pages::PageRouter;
use bough_x::damage_engine::{BlurLayer, DamageEngine, DamageReport, OpaqueCover, Rect, Work};

/// Structured inventory of all active visual effects in a `FlatScene`, for `effects_inspect`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EffectsSummary {
    pub drop_shadows: u32,
    pub inset_shadows: u32,
    pub linear_gradients: u32,
    pub radial_gradients: u32,
    pub conic_gradients: u32,
    pub backdrop_blurs: u32,
    pub retained_groups: u32,
    pub total_nodes: u32,
}

/// Inspect a `FlatScene` and summarize every active effect primitive without mutating scene state.
pub fn inspect_flat_effects(scene: &FlatScene) -> EffectsSummary {
    let mut out = EffectsSummary::default();
    let count = scene.nodes();
    out.total_nodes = count as u32;
    for i in 0..count {
        let n = scene.node(NodeId(i as u32));
        match &n.cmd {
            DrawCmd::Shadow { .. } => out.drop_shadows += 1,
            DrawCmd::Inset { .. } => out.inset_shadows += 1,
            DrawCmd::Ramp { geo, .. } => match geo {
                RampGeo::Linear { .. } => out.linear_gradients += 1,
                RampGeo::Radial { .. } => out.radial_gradients += 1,
                RampGeo::Conic { .. } => out.conic_gradients += 1,
            },
            DrawCmd::Glass { .. } => out.backdrop_blurs += 1,
            DrawCmd::Group { .. } => out.retained_groups += 1,
            _ => {}
        }
    }
    out
}

/// Synchronize opaque covers and backdrop-blur layers from a `FlatScene` into a `DamageEngine`
/// so `DamageEngine` can perform front-to-back occlusion culling, blur mip-dilation, rigid scroll
/// blits, and adaptive CPU/Metal work-model selection for the scene.
pub fn sync_flat_scene_to_damage_engine(scene: &FlatScene, engine: &mut DamageEngine) {
    engine.covers_clear();
    engine.layers_clear();
    let count = scene.nodes();
    let mut layer_id = 0u32;
    for i in 0..count {
        let n = scene.node(NodeId(i as u32));
        let z = i as u32;
        let rect = Rect::new(
            n.rect[0].floor() as i32,
            n.rect[1].floor() as i32,
            n.rect[2].ceil() as i32,
            n.rect[3].ceil() as i32,
        );
        match &n.cmd {
            DrawCmd::Solid { color } if (*color >> 24) == 255 && n.opacity >= 0.999 => {
                engine.cover_push(OpaqueCover { z, core_rect: rect });
            }
            DrawCmd::Glass { blur, .. } => {
                layer_id += 1;
                engine.layer_push(BlurLayer {
                    id: layer_id,
                    z,
                    rect,
                    radius: blur.ceil().max(1.0) as i32,
                    max_level: 3,
                });
            }
            _ => {}
        }
    }
    engine.covers_commit();
    engine.layers_commit();
}

/// Unified frame telemetry combining `DamageEngine`, `FlatScene`, `Motion`, and `PageRouter`.
#[derive(Clone, Debug)]
pub struct UnifiedFrameTelemetry {
    pub damage: DamageReport,
    pub effects: EffectsSummary,
    pub router_in_flight: bool,
    pub motion_animating: bool,
}

/// Evaluate one unified frame step: synchronizes `FlatScene` effects/covers into `DamageEngine`,
/// advances `PageRouter` at monotonic `now`, resolves damage, and returns unified telemetry
/// suitable for `bough-inspect` and `bough-mcp`.
pub fn step_unified_telemetry(
    scene: &FlatScene,
    engine: &mut DamageEngine,
    router: &mut PageRouter,
    motion: &Motion,
    changed_rects: &[(u32, Rect, Work)],
    now: ClockNs,
) -> UnifiedFrameTelemetry {
    engine.begin_frame();
    sync_flat_scene_to_damage_engine(scene, engine);
    for &(z, rect, work) in changed_rects {
        engine.damage(z, rect, work);
    }
    engine.resolve(&mut |_layer, rect| Some(rect));
    let _ = router.tick(now);
    UnifiedFrameTelemetry {
        damage: engine.report(),
        effects: inspect_flat_effects(scene),
        router_in_flight: router.in_flight(),
        motion_animating: motion.animating(),
    }
}
