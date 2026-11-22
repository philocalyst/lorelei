//! The rich telemetry endpoints, driven through the same `rpc::handle` admission the socket and the MCP server
//! use: the damage map (with an off-thread heatmap PNG), effects, animations, scrubbing, the frame profile,
//! selector assertions and the batch envelope.
use bough_guibench_dataflow::rows::Rows;
use bough_inspect::json::{self, Value as J};
use bough_inspect::rpc;
use bough_inspect::session::Session;
use bough_inspect_app::Lab;

fn call(s: &mut Session<Lab<Rows>>, method: &str, params: J) -> Result<J, String> {
    match rpc::handle(s, method, &params).result {
        Ok(v) => Ok(v),
        Err(e) => Err(format!("{}: {}", e.code, e.message)),
    }
}

#[test]
fn damage_map_reports_stats_and_an_off_thread_heatmap() {
    let mut s = Session::new(Lab::new(Rows::new(2_000), 800, 600, 1));
    // produce damage: a scroll write marks tiles
    let _ = s.set("scroll", &bough_inspect::PropValue::Int(64));
    let v = call(
        &mut s,
        "damage_map",
        J::obj().set("include_heatmap_png", true),
    )
    .expect("damage_map");
    assert!(v.get("repair_pct").is_some(), "{v:?}");
    assert_eq!(v.get("viewport_pixels").and_then(J::as_u32), Some(800 * 600));
    assert!(v.get("damaged_tiles").is_some());
    let b64 = v
        .get("heatmap_png_base64")
        .and_then(J::as_str)
        .expect("the heatmap was requested");
    let img = bough_inspect::png::decode(&json::base64_decode(b64).expect("base64")).expect("a PNG");
    assert_eq!((img.w, img.h), (800, 600));
}

#[test]
fn assert_selector_passes_and_fails() {
    let mut s = Session::new(Lab::new(Rows::new(2_000), 800, 600, 1));
    let pass = call(
        &mut s,
        "assert_selector",
        J::obj().set("kind", "text").set("min_count", 1),
    )
    .expect("assert_selector");
    assert_eq!(pass.get("pass").and_then(J::as_bool), Some(true), "{pass:?}");
    assert!(pass.get("matched").and_then(J::as_u32).unwrap_or(0) >= 1);

    let fail = call(
        &mut s,
        "assert_selector",
        J::obj().set("kind", "no_such_kind").set("exact_count", 1),
    )
    .expect("assert_selector");
    assert_eq!(fail.get("pass").and_then(J::as_bool), Some(false), "{fail:?}");
    assert!(!fail.get("diagnostics").map(J::arr).unwrap_or(&[]).is_empty());
}

#[test]
fn batch_runs_steps_and_stops_on_error() {
    let mut s = Session::new(Lab::new(Rows::new(500), 800, 600, 1));
    let steps = J::Arr(vec![
        J::obj().set("method", "counters").set("params", J::obj()),
        J::obj().set("method", "profile_frame").set("params", J::obj()),
    ]);
    let v = call(&mut s, "batch", J::obj().set("steps", steps)).expect("batch");
    let steps = v.get("steps").map(J::arr).unwrap_or(&[]);
    assert_eq!(steps.len(), 2, "{v:?}");
    assert_eq!(steps[0].get("ok").and_then(J::as_bool), Some(true));
    assert_eq!(steps[1].get("ok").and_then(J::as_bool), Some(true));
    assert!(v.get("total_us").is_some());

    // stop_on_error: the first step fails, the second is never run
    let steps = J::Arr(vec![
        J::obj().set("method", "no_such_method").set("params", J::obj()),
        J::obj().set("method", "profile_frame").set("params", J::obj()),
    ]);
    let v = call(
        &mut s,
        "batch",
        J::obj().set("steps", steps).set("stop_on_error", true),
    )
    .expect("batch");
    assert_eq!(v.get("steps").map(J::arr).unwrap_or(&[]).len(), 1, "{v:?}");
    assert_eq!(
        v.get("steps").map(J::arr).unwrap_or(&[])[0].get("ok").and_then(J::as_bool),
        Some(false)
    );
}

#[test]
fn effects_animations_profile_and_scrub_answer() {
    let mut s = Session::new(Lab::new(Rows::new(500), 800, 600, 1));
    let fx = call(&mut s, "effects_inspect", J::obj()).expect("effects_inspect");
    assert!(fx.get("shadows").is_some());
    assert!(fx.get("gradients").is_some());
    assert!(fx.get("isolated_groups").is_some());

    let anim = call(&mut s, "animations", J::obj()).expect("animations");
    assert!(anim.get("animations").is_some(), "{anim:?}");

    let prof = call(&mut s, "profile_frame", J::obj()).expect("profile_frame");
    assert!(prof.get("total_us").is_some());
    assert!(prof.get("executor").and_then(J::as_str).is_some());
    assert!(prof.get("phases").is_some());

    // the default has no animation bank: scrubbing reports that honestly, not an error
    let scrub = call(&mut s, "scrub_animation", J::obj().set("op", "pause")).expect("scrub");
    assert_eq!(scrub.get("unsupported").and_then(J::as_bool), Some(true), "{scrub:?}");
    let scrub = call(&mut s, "scrub_animation", J::obj().set("op", "seek_id").set("id", "0")).expect("scrub");
    assert!(scrub.get("unsupported").is_some());
}
