//! The rich telemetry endpoints: damage maps (with an optional off-thread heatmap PNG), effects, animations and
//! scrubbing, per-phase frame profiles, selector assertions and a batch envelope. The socket and the RPC
//! dispatcher both call these, so a tool and a test drive exactly the same admission.
use crate::json::Value as J;
use crate::protocol::{
    Animation, Assertion, BatchResult, BatchStepResult, DamageMap, EffectsInspect, FrameProfile, ScrubCommand,
    ScrubResult, SelectorAssertion,
};
use crate::rpc::RpcError;
use crate::session::{Program, Session};
use std::sync::Arc;
use std::time::Instant;

/// A `damage_map` result: either ready now, or awaiting an off-thread heatmap PNG encode.
pub enum DamageReply {
    Ready(J),
    Heatmap {
        json: J,
        w: i32,
        h: i32,
        px: Arc<[u32]>,
    },
}

fn flag(p: &J, k: &str, d: bool) -> bool {
    p.get(k).and_then(J::as_bool).unwrap_or(d)
}

fn str_of(p: &J, k: &str) -> Option<String> {
    p.get(k).and_then(J::as_str).map(str::to_owned)
}

fn usize_of(p: &J, k: &str) -> Option<usize> {
    p.get(k).and_then(J::as_usize)
}

fn i32_of(p: &J, k: &str) -> Option<i32> {
    p.get(k)
        .and_then(J::as_i64)
        .and_then(|x| i32::try_from(x).ok())
}

fn f64_of(p: &J, k: &str) -> Option<f64> {
    p.get(k).and_then(J::as_f64)
}

/// The live damage map, from the integration's hook (with a snapshot-derived default).
pub fn damage_map<P: Program>(s: &mut Session<P>, p: &J) -> Result<DamageReply, RpcError> {
    let snap = s.snapshot().clone();
    let map = s.app().damage_map(&snap);
    let json = map.json();
    if flag(p, "include_heatmap_png", false) {
        let w = snap.app.w.max(0);
        let h = snap.app.h.max(0);
        let px = render_heatmap(w, h, &map);
        Ok(DamageReply::Heatmap {
            json,
            w,
            h,
            px: Arc::from(px),
        })
    } else {
        Ok(DamageReply::Ready(json))
    }
}

/// Complete a `damage_map` reply by encoding the heatmap on the calling thread (the socket defers this; the
/// direct RPC path runs it here).
pub fn finish_damage_reply(reply: DamageReply) -> J {
    match reply {
        DamageReply::Ready(json) => json,
        DamageReply::Heatmap { mut json, w, h, px } => {
            let encoded = crate::encoder::encode(w, h, &px, false);
            json.put("heatmap_png_base64", encoded.png_base64.as_str());
            json
        }
    }
}

/// Render a colour-coded overlay: cold/warm/hot tiles plus dirty-rect outlines, over a transparent frame.
pub fn render_heatmap(w: i32, h: i32, map: &DamageMap) -> Vec<u32> {
    const TILE: i32 = 8;
    let (w, h) = (w.max(0), h.max(0));
    let mut px = vec![0u32; (w as usize).saturating_mul(h as usize)];
    let cold = 0x8040_a0ffu32;
    let warm = 0xffff_b030u32;
    let hot = 0xffff_4030u32;
    for t in &map.heat {
        let color = if t.repaints >= 2 {
            hot
        } else if t.repaints == 1 {
            warm
        } else {
            cold
        };
        let (x0, y0) = (t.x * TILE, t.y * TILE);
        for y in y0.max(0)..(y0 + TILE).min(h) {
            for x in x0.max(0)..(x0 + TILE).min(w) {
                if x >= 0 && y >= 0 {
                    px[(y * w + x) as usize] = color;
                }
            }
        }
    }
    // dirty rects as a two-pixel outline
    for r in &map.rects {
        let (x0, y0, x1, y1) = (r.x0.max(0), r.y0.max(0), r.x1.min(w), r.y1.min(h));
        if x1 <= x0 || y1 <= y0 {
            continue;
        }
        for x in x0..x1 {
            px[(y0 * w + x) as usize] = 0xffff_4040;
            px[((y1 - 1) * w + x) as usize] = 0xffff_4040;
        }
        for y in y0..y1 {
            px[(y * w + x0) as usize] = 0xffff_4040;
            px[(y * w + x1 - 1) as usize] = 0xffff_4040;
        }
    }
    px
}

/// Active effects for a node or the whole scene.
pub fn effects_inspect<P: Program>(
    s: &mut Session<P>,
    p: &J,
) -> Result<EffectsInspect, RpcError> {
    let snap = s.snapshot().clone();
    let node = usize_of(p, "node").or_else(|| p.get("node_id").and_then(J::as_usize));
    if let Some(n) = node
        && n >= snap.nodes.len()
    {
        return Err(RpcError::params("node index out of range"));
    }
    Ok(s.app().effects_inspect(&snap, node))
}

/// All live animations and page transitions.
pub fn animations<P: Program>(s: &mut Session<P>) -> Vec<Animation> {
    let snap = s.snapshot().clone();
    s.app().animations(&snap)
}

/// Parse and run one deterministic scrub command.
pub fn scrub_animation<P: Program>(
    s: &mut Session<P>,
    p: &J,
) -> Result<ScrubResult, RpcError> {
    let snap = s.snapshot().clone();
    let op = str_of(p, "op")
        .or_else(|| str_of(p, "action"))
        .unwrap_or_else(|| "list".to_owned());
    let cmd = ScrubCommand {
        ms: f64_of(p, "ms").or_else(|| f64_of(p, "time_ms")),
        id: usize_of(p, "id").map(|x| x as u64).or_else(|| {
            p.get("id")
                .and_then(J::as_str)
                .and_then(|s| u64::from_str_radix(s, 16).ok())
        }),
        op,
    };
    Ok(s.app_mut().scrub_animation(&snap, &cmd))
}

/// A structured per-phase frame profile.
pub fn profile_frame<P: Program>(s: &mut Session<P>) -> FrameProfile {
    let snap = s.snapshot().clone();
    s.app().profile_frame(&snap)
}

/// Parse a selector assertion from its JSON form.
pub fn selector_from_json(p: &J) -> Result<SelectorAssertion, RpcError> {
    Ok(SelectorAssertion {
        kind: str_of(p, "kind"),
        text: str_of(p, "text"),
        ty: str_of(p, "ty"),
        within: usize_of(p, "within"),
        visible: p.get("visible").and_then(J::as_bool),
        min_count: usize_of(p, "min_count"),
        max_count: usize_of(p, "max_count"),
        exact_count: usize_of(p, "exact_count"),
        text_contains: str_of(p, "text_contains"),
        text_equals: str_of(p, "text_equals"),
        x_min: i32_of(p, "x_min"),
        x_max: i32_of(p, "x_max"),
        y_min: i32_of(p, "y_min"),
        y_max: i32_of(p, "y_max"),
        w_min: i32_of(p, "w_min"),
        w_max: i32_of(p, "w_max"),
        h_min: i32_of(p, "h_min"),
        h_max: i32_of(p, "h_max"),
        style: str_of(p, "style"),
        style_equals: str_of(p, "style_equals"),
    })
}

/// Run one selector assertion against the snapshot. Pure: it never writes state.
pub fn assert_selector<P: Program>(
    s: &mut Session<P>,
    p: &J,
) -> Result<Assertion, RpcError> {
    let snap = s.snapshot().clone();
    let sel = selector_from_json(p)?;
    let mut diagnostics = Vec::new();
    let mut matched = 0usize;
    let mut first = None;
    for (i, n) in snap.nodes.iter().enumerate() {
        if let Some(k) = &sel.kind
            && n.kind != k
        {
            continue;
        }
        if let Some(t) = &sel.ty
            && !n.ty.contains(t.as_str())
        {
            continue;
        }
        if let Some(t) = &sel.text
            && !n.text.as_deref().is_some_and(|x| x.contains(t.as_str()))
        {
            continue;
        }
        if let Some(w) = sel.within
            && n.store != w
        {
            continue;
        }
        if let Some(v) = sel.visible
            && n.visible != v
        {
            continue;
        }
        matched += 1;
        if first.is_none() {
            first = Some(i);
        }
    }
    let mut pass = true;
    if let Some(min) = sel.min_count {
        if matched < min {
            pass = false;
            diagnostics.push(format!("matched {matched}, expected at least {min}"));
        }
    }
    if let Some(max) = sel.max_count {
        if matched > max {
            pass = false;
            diagnostics.push(format!("matched {matched}, expected at most {max}"));
        }
    }
    if let Some(exact) = sel.exact_count {
        if matched != exact {
            pass = false;
            diagnostics.push(format!("matched {matched}, expected exactly {exact}"));
        }
    }
    // text and bounds invariants apply to the first match, when the selector names content.
    if let Some(i) = first {
        let n = &snap.nodes[i];
        if let Some(contains) = &sel.text_contains {
            let got = n.text.as_deref().unwrap_or("");
            if !got.contains(contains.as_str()) {
                pass = false;
                diagnostics.push(format!("text {got:?} does not contain {contains:?}"));
            }
        }
        if let Some(equals) = &sel.text_equals {
            let got = n.text.as_deref().unwrap_or("");
            if got != equals {
                pass = false;
                diagnostics.push(format!("text {got:?} is not {equals:?}"));
            }
        }
        let r = n.screen;
        let w = r.w();
        let h = r.h();
        for (cond, what) in [
            (sel.x_min.map(|m| r.x0 < m), "x_min"),
            (sel.x_max.map(|m| r.x1 > m), "x_max"),
            (sel.y_min.map(|m| r.y0 < m), "y_min"),
            (sel.y_max.map(|m| r.y1 > m), "y_max"),
            (sel.w_min.map(|m| w < m), "w_min"),
            (sel.w_max.map(|m| w > m), "w_max"),
            (sel.h_min.map(|m| h < m), "h_min"),
            (sel.h_max.map(|m| h > m), "h_max"),
        ] {
            if cond == Some(true) {
                pass = false;
                diagnostics.push(format!(
                    "bounds {what}: node {i} is x={} y={} w={w} h={h}",
                    r.x0, r.y0
                ));
            }
        }
        if let (Some(style), Some(expected)) = (&sel.style, &sel.style_equals) {
            let got = n.prop(style).map(|p| p.value.text()).unwrap_or_default();
            if got != *expected {
                pass = false;
                diagnostics.push(format!("style {style:?} is {got:?}, expected {expected:?}"));
            }
        }
    } else if sel.text_contains.is_some()
        || sel.text_equals.is_some()
        || sel.w_min.is_some()
        || sel.h_min.is_some()
    {
        pass = false;
        diagnostics.push("no node matched; text and bounds invariants cannot be checked".into());
    }
    Ok(Assertion {
        pass,
        matched,
        diagnostics,
        first_node: first,
    })
}

/// Run a sequence of RPC commands in one round-trip, with per-step results.
pub fn batch<P: Program>(s: &mut Session<P>, p: &J) -> Result<BatchResult, RpcError> {
    let steps = p
        .get("steps")
        .ok_or_else(|| RpcError::params("steps"))?
        .arr();
    if steps.is_empty() {
        return Err(RpcError::params("steps must not be empty"));
    }
    if steps.len() > 256 {
        return Err(RpcError::params("at most 256 steps"));
    }
    let stop_on_error = flag(p, "stop_on_error", false);
    let start = Instant::now();
    let mut out = Vec::with_capacity(steps.len());
    for (index, step) in steps.iter().enumerate() {
        let method = step
            .get("method")
            .and_then(J::as_str)
            .ok_or_else(|| RpcError::params(&format!("steps[{index}].method")))?;
        let params = step.get("params").cloned().unwrap_or(J::Null);
        let reply = crate::rpc::handle(s, method, &params);
        let result = reply.result;
        let ok = result.is_ok();
        if stop_on_error && !ok {
            out.push(BatchStepResult {
                index,
                method: method.to_owned(),
                ok,
                result: result.as_ref().ok().cloned().unwrap_or(J::Null),
                error: result.err().map(|e| e.to_json()),
            });
            return Ok(BatchResult {
                steps: out,
                total_us: start.elapsed().as_micros() as u64,
                stop_on_error,
            });
        }
        out.push(BatchStepResult {
            index,
            method: method.to_owned(),
            ok,
            result: result.as_ref().ok().cloned().unwrap_or(J::Null),
            error: result.err().map(|e| e.to_json()),
        });
    }
    Ok(BatchResult {
        steps: out,
        total_us: start.elapsed().as_micros() as u64,
        stop_on_error,
    })
}
