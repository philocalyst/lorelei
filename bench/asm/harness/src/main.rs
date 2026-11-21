//! Instantiates the hot monomorphisations the asm harness inspects.
//!
//! A generic function has no symbol until a type instantiates it, and `cargo asm` reads symbols. This
//! binary drives the new core exactly as the gui-bench app's headless ring does (mount, settle, paint,
//! input), builds the layered canvas scene, shades tiles and picks — so the release build emits the
//! bodies of `World<Rows, Spans>::settle`, `changed`, `delta`, `paint`, the span raster, the tile
//! raster, and the expression compare. Nothing is timed here; the binary is thrown away after it makes
//! the symbols, and `main` only keeps them alive through `black_box`.
use std::hint::black_box;

use bough_expr::U;
use bough_guibench_x::rows::Rows;
use bough_guibench_x::scene::Scene as AppScene;
use bough_host::App;
use bough_kernel::cpu::Pool;
use bough_paint::scenes;

// The API surfaces a turn actually crosses. Each wrapper is `inline(never)` so the release build
// emits one symbol whose body is the inlined implementation: `cargo asm` reads symbols, and a
// function inlined into every caller has none. The wrapper's own call/return is a few instructions
// and is visible in the listing.
/// The negative control: with `--cfg asm_mutant`, each wrapper does real extra work. The harness
/// must see it in the instruction count (a count that cannot move is not measuring anything).
#[cfg(feature = "asm_mutant")]
#[inline(never)]
fn extra(mut x: u64) -> u64 {
    for i in 0..64u64 {
        x = black_box(x.wrapping_mul(31).wrapping_add(i));
    }
    x
}
#[cfg(not(feature = "asm_mutant"))]
#[inline(always)]
fn extra(x: u64) -> u64 {
    x
}

#[inline(never)]
fn w_resize(app: &mut AppScene<Rows>) {
    app.resize(1280, 800, 1);
}
#[inline(never)]
fn w_settle(app: &mut AppScene<Rows>) -> usize {
    let n = app.settle().bits.len();
    (n as u64 + extra(n as u64)) as usize
}
#[inline(never)]
fn w_paint(app: &mut AppScene<Rows>, px: &mut [u32]) {
    // SAFETY: `px` holds exactly the window's rows at the stride given, and nothing else touches it.
    unsafe { app.paint(px.as_mut_ptr(), 1280, None) }
    black_box(extra(px.len() as u64));
}
#[inline(never)]
fn w_shade(paint: &bough_paint::scene::Scene, tiles: &[u32], out: &mut [u32]) -> u64 {
    let pool = Pool::new(0);
    let n = paint.shade(&U::default(), tiles, out, paint.fw as usize, &pool, 0).entries;
    n + extra(n)
}
#[inline(never)]
fn w_pick(paint: &bough_paint::scene::Scene) -> u32 {
    paint.pick(&U::default(), 10, 10)
}

// The façade's sheet row through the layout memo (the shape `bough-lay` was built for).
#[path = "../../../../crates/bough-lay/scenes/rows.rs"]
mod layrows;

#[inline(never)]
fn w_layout() -> i32 {
    let mut memo = bough_lay::Memo::new();
    let mut out = [[0i32; 4]; 22];
    memo.row(&layrows::sheet_row(1), 2.0, &U::default(), 2560.0, (0, 0), &mut out[..])
}

fn main() {
    // The app's ring: a world of 1,000 rows, mounted, settled twice, painted in full, and painted by damage.
    let mut app = AppScene::new(Rows::new(1_000));
    w_resize(&mut app);
    let damaged = w_settle(&mut app);
    let mut px = vec![0u32; 1280 * 800];
    w_paint(&mut app, &mut px);
    app.painted();
    black_box(damaged);

    // A pointer move at a row: the pick path, the handlers' compare and the second settle.
    app.input(&bough_host::event::Event::Motion { x: 40.0, y: 5.0 * 32.0 + 16.0 });
    app.settle();
    black_box(&px);

    // The layered canvas: the per-archetype tile raster and the pick, over every tile.
    let (paint, _model) = scenes::canvas::build(1);
    let mut out = vec![0u32; paint.fw as usize * paint.fh as usize];
    let tiles: Vec<u32> = (0..(paint.tx * paint.ty) as u32).collect();
    let entries = w_shade(&paint, &tiles, &mut out);
    let picked = w_pick(&paint);
    black_box((entries, picked, &out));

    black_box(w_layout());

    // The state block's compare: the one pass that notices a write.
    let a = U::default();
    let mut b = U::default();
    b.w[3] = 1;
    black_box(a.diff(&b, 64));
}
