//! The inspector's rich telemetry model: damage maps, effects inventories, animation state and scrubbing,
//! per-phase frame profiles, selector assertions and a batch envelope. Each struct has a canonical `json`
//! projection so the socket, the MCP server and a test share one serialization.
use crate::json::{self, Value as J};
use crate::schema::R;

// ---- damage map -----------------------------------------------------------------------------------------------

/// One tile's accumulated repaint count, in tile coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct TileHeat {
    pub x: i32,
    pub y: i32,
    pub repaints: u32,
}

/// A scroll blit: the sweep the last turn produced, if any.
#[derive(Clone, Debug, PartialEq)]
pub struct Blit {
    pub col: u32,
    pub until: u32,
    pub rect: R,
    pub origin: (i32, i32),
}

/// The damage the last committed turn produced, summarised for an agent.
#[derive(Clone, Debug, PartialEq)]
pub struct DamageMap {
    pub rects: Vec<R>,
    pub damaged_tiles: u32,
    pub damaged_pixels: u64,
    pub viewport_pixels: u64,
    /// `damaged_pixels / viewport_pixels * 100`.
    pub repair_pct: f64,
    /// Hex words of the tile bitmap, one string per row (`rows[].y` is implied by position here).
    pub tile_bitmap_words: Vec<String>,
    pub heat: Vec<TileHeat>,
    pub scroll_blit: Option<Blit>,
    pub occluded_tiles: u32,
    pub backdrop_dilated_tiles: u32,
    pub heatmap_png_base64: Option<String>,
}

impl DamageMap {
    pub fn json(&self) -> J {
        J::obj()
            .set("rects", self.rects.iter().map(R::json).collect::<Vec<_>>())
            .set("damaged_tiles", self.damaged_tiles)
            .set("damaged_pixels", self.damaged_pixels)
            .set("viewport_pixels", self.viewport_pixels)
            .set("repair_pct", self.repair_pct)
            .set(
                "tile_bitmap_words",
                self.tile_bitmap_words
                    .iter()
                    .map(|w| J::Str(w.clone()))
                    .collect::<Vec<_>>(),
            )
            .set(
                "heat",
                self.heat
                    .iter()
                    .map(|h| {
                        J::obj()
                            .set("x", h.x)
                            .set("y", h.y)
                            .set("repaints", h.repaints)
                    })
                    .collect::<Vec<_>>(),
            )
            .set("scroll_blit", self.scroll_blit.as_ref().map_or(J::Null, |b| {
                J::obj()
                    .set("col", b.col)
                    .set("until", b.until)
                    .set("rect", b.rect.json())
                    .set(
                        "origin",
                        vec![J::Int(b.origin.0 as i64), J::Int(b.origin.1 as i64)],
                    )
            }))
            .set("occluded_tiles", self.occluded_tiles)
            .set("backdrop_dilated_tiles", self.backdrop_dilated_tiles)
            .set(
                "heatmap_png_base64",
                self.heatmap_png_base64
                    .as_ref()
                    .map_or(J::Null, |s| J::Str(s.clone())),
            )
    }
}

// ---- effects --------------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct Shadow {
    pub inset: bool,
    pub radius: f32,
    pub spread: f32,
    pub offset: (f32, f32),
    pub color: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Blur {
    pub backdrop: bool,
    pub radius: f32,
    pub mip_levels: u32,
    pub bbox: R,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Gradient {
    pub kind: &'static str,
    pub stops: u32,
    pub angle: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EffectsInspect {
    /// The node the effects belong to, when one was selected; `None` is the whole scene.
    pub node: Option<usize>,
    pub shadows: Vec<Shadow>,
    pub blurs: Vec<Blur>,
    pub gradients: Vec<Gradient>,
    /// Node indices that form isolated opacity groups.
    pub isolated_groups: Vec<usize>,
}

impl EffectsInspect {
    pub fn json(&self) -> J {
        J::obj()
            .set("node", self.node.map_or(J::Null, |n| J::Int(n as i64)))
            .set(
                "shadows",
                self.shadows
                    .iter()
                    .map(|s| {
                        J::obj()
                            .set("inset", s.inset)
                            .set("radius", s.radius)
                            .set("spread", s.spread)
                            .set(
                                "offset",
                                vec![J::from(s.offset.0), J::from(s.offset.1)],
                            )
                            .set("color", s.color)
                    })
                    .collect::<Vec<_>>(),
            )
            .set(
                "blurs",
                self.blurs
                    .iter()
                    .map(|b| {
                        J::obj()
                            .set("backdrop", b.backdrop)
                            .set("radius", b.radius)
                            .set("mip_levels", b.mip_levels)
                            .set("bbox", b.bbox.json())
                    })
                    .collect::<Vec<_>>(),
            )
            .set(
                "gradients",
                self.gradients
                    .iter()
                    .map(|g| {
                        J::obj()
                            .set("kind", g.kind)
                            .set("stops", g.stops)
                            .set("angle", g.angle)
                    })
                    .collect::<Vec<_>>(),
            )
            .set(
                "isolated_groups",
                self.isolated_groups
                    .iter()
                    .map(|&i| J::Int(i as i64))
                    .collect::<Vec<_>>(),
            )
    }
}

// ---- animations -----------------------------------------------------------------------------------------------

/// One live animation or page transition.
#[derive(Clone, Debug, PartialEq)]
pub struct Animation {
    pub id: u64,
    /// The stable node id being animated, when it is bound to one record.
    pub node: Option<u64>,
    pub property: String,
    /// `spring`, `cubic-bezier`, `keyframe`, `transition`.
    pub kind: String,
    pub elapsed_ms: f64,
    pub duration_ms: Option<f64>,
    pub value: f64,
    pub velocity: f64,
    pub settled: bool,
    /// The page transition phase (`enter`, `exit`, `idle`, ...).
    pub phase: String,
}

impl Animation {
    pub fn json(&self) -> J {
        J::obj()
            .set("id", format!("{:016x}", self.id))
            .set("node", self.node.map_or(J::Null, |n| J::Str(format!("{:016x}", n))))
            .set("property", self.property.as_str())
            .set("kind", self.kind.as_str())
            .set("elapsed_ms", self.elapsed_ms)
            .set("duration_ms", self.duration_ms.map_or(J::Null, J::from))
            .set("value", self.value)
            .set("velocity", self.velocity)
            .set("settled", self.settled)
            .set("phase", self.phase.as_str())
    }
}

/// One deterministic scrub command.
#[derive(Clone, Debug, PartialEq)]
pub struct ScrubCommand {
    /// `set_time_ms`, `advance_ms`, `pause`, `resume`, `seek_id`.
    pub op: String,
    pub ms: Option<f64>,
    pub id: Option<u64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ScrubResult {
    pub ok: bool,
    pub time_ms: f64,
    pub paused: bool,
    pub settled: Vec<u64>,
    pub unsupported: bool,
}

impl ScrubResult {
    pub fn json(&self) -> J {
        J::obj()
            .set("ok", self.ok)
            .set("time_ms", self.time_ms)
            .set("paused", self.paused)
            .set(
                "settled",
                self.settled
                    .iter()
                    .map(|&id| J::Str(format!("{id:016x}")))
                    .collect::<Vec<_>>(),
            )
            .set("unsupported", self.unsupported)
    }
}

// ---- frame profile --------------------------------------------------------------------------------------------

/// One phase of a frame, in microseconds.
#[derive(Clone, Debug, PartialEq)]
pub struct Phase {
    pub name: &'static str,
    pub us: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FrameProfile {
    pub input_us: u64,
    pub eval_us: u64,
    pub layout_us: u64,
    pub damage_us: u64,
    pub paint_us: u64,
    pub submit_us: u64,
    pub total_us: u64,
    pub allocated_bytes_warm: u64,
    pub dirty_nodes: usize,
    pub dirty_tiles: u32,
    /// `cpu`, `metal`, `mixed`.
    pub executor: &'static str,
    pub phases: Vec<Phase>,
}

impl FrameProfile {
    pub fn json(&self) -> J {
        J::obj()
            .set(
                "phases",
                self.phases
                    .iter()
                    .map(|p| J::obj().set("name", p.name).set("us", p.us))
                    .collect::<Vec<_>>(),
            )
            .set("input_us", self.input_us)
            .set("eval_us", self.eval_us)
            .set("layout_us", self.layout_us)
            .set("damage_us", self.damage_us)
            .set("paint_us", self.paint_us)
            .set("submit_us", self.submit_us)
            .set("total_us", self.total_us)
            .set("allocated_bytes_warm", self.allocated_bytes_warm)
            .set("dirty_node_count", self.dirty_nodes)
            .set("dirty_tile_count", self.dirty_tiles)
            .set("executor", self.executor)
    }
}

// ---- selector assertion ---------------------------------------------------------------------------------------

/// What a selector asserts, and the bounds it may check.
#[derive(Clone, Debug, PartialEq)]
pub struct SelectorAssertion {
    pub kind: Option<String>,
    pub text: Option<String>,
    pub ty: Option<String>,
    pub within: Option<usize>,
    pub visible: Option<bool>,
    pub min_count: Option<usize>,
    pub max_count: Option<usize>,
    pub exact_count: Option<usize>,
    pub text_contains: Option<String>,
    pub text_equals: Option<String>,
    pub x_min: Option<i32>,
    pub x_max: Option<i32>,
    pub y_min: Option<i32>,
    pub y_max: Option<i32>,
    pub w_min: Option<i32>,
    pub w_max: Option<i32>,
    pub h_min: Option<i32>,
    pub h_max: Option<i32>,
    pub style: Option<String>,
    pub style_equals: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Assertion {
    pub pass: bool,
    pub matched: usize,
    pub diagnostics: Vec<String>,
    pub first_node: Option<usize>,
}

impl Assertion {
    pub fn json(&self) -> J {
        J::obj()
            .set("pass", self.pass)
            .set("matched", self.matched)
            .set("first_node", self.first_node.map_or(J::Null, |n| J::Int(n as i64)))
            .set(
                "diagnostics",
                self.diagnostics
                    .iter()
                    .map(|d| J::Str(d.clone()))
                    .collect::<Vec<_>>(),
            )
    }
}

// ---- batch ----------------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct BatchStep {
    pub method: String,
    pub params: J,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BatchStepResult {
    pub index: usize,
    pub method: String,
    pub ok: bool,
    pub result: J,
    pub error: Option<J>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BatchResult {
    pub steps: Vec<BatchStepResult>,
    pub total_us: u64,
    pub stop_on_error: bool,
}

impl BatchResult {
    pub fn json(&self) -> J {
        J::obj()
            .set(
                "steps",
                self.steps
                    .iter()
                    .map(|s| {
                        J::obj()
                            .set("index", s.index)
                            .set("method", s.method.as_str())
                            .set("ok", s.ok)
                            .set("result", s.result.clone())
                            .set("error", s.error.clone().unwrap_or(J::Null))
                    })
                    .collect::<Vec<_>>(),
            )
            .set("total_us", self.total_us)
            .set("stop_on_error", self.stop_on_error)
    }
}

/// A small JSON helper for building results.
pub fn json_string(v: &J) -> String {
    json::to_string(v)
}
