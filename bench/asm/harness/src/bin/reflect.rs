//! Dumps the new core's learned expression dependency graph for the gui-bench `rows` scene.
//!
//! One line per column: store path, column, paint kind, stage, the 64-bit mask of state-block words it
//! reads, its operand names, and its record type. `bench/asm/asm.py graph expr` turns the lines into
//! Graphviz. Uses only the public reflection API (`bough_dataflow::reflect::Reflect`).

use bough_guibench_dataflow::rows::Rows;
use bough_guibench_dataflow::scene::Scene;
use bough_host::App;
use bough_dataflow::reflect::{Kind, Reflect, StoreInfo};

fn walk(info: &StoreInfo, path: &str) {
    for (i, col) in info.cols.iter().enumerate() {
        let Some(c) = col else { continue };
        let ops: Vec<&str> = c.ops.iter().map(|o| o.name).collect();
        println!(
            "{path}\t{i}\t{}\t{:?}\t{:016x}\t{}\t{}",
            c.kind,
            c.stage,
            c.reads,
            ops.join(","),
            c.ty
        );
    }
    for (q, sub) in info.subs.iter().enumerate() {
        if matches!(sub.kind, Kind::Root) {
            continue;
        }
        walk(sub, &format!("{path}.{q}"));
    }
}

fn main() {
    let mut app = Scene::new(Rows::new(1_000));
    app.resize(1280, 800, 1);
    app.settle();
    let r = Reflect::learn(&app.world, &app.v);
    println!("# store\tcol\tkind\tstage\treads\tops\ttype");
    walk(&r.root, "root");
}
