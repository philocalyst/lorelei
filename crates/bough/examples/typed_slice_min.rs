//! The vertical slice mounted once on the typed-dataflow path and on the lowered one: the compile-cost
//! counterparts of `flat_slice.rs` (which mounts the slice written against the flat core). Nothing is measured
//! at run time here; `--emit=llvm-ir` and `-Z time-passes` on these three files is the measurement.
#[path = "slice.rs"]
#[allow(dead_code)]
mod slice;

fn main() {
    let cfg = || bough::raw::Config::new(640.0, 480.0, 2.0, bough::ui::default_font());
    if std::env::args().any(|a| a == "lowered") {
        let mut app = bough::lower::App::new(slice::Slice::new(1000), cfg());
        app.frame();
        println!("lowered slice: {} records", app.record_count());
        return;
    }
    match bough::typed::App::new(slice::Slice::new(1000), cfg()) {
        Ok(mut app) => {
            app.frame();
            println!("typed slice mounted");
        }
        Err((_, why)) => println!("not admitted: {why:?}"),
    }
}
