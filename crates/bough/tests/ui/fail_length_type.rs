// A length is points as `f32` or an integer.
use bough::prelude::*;
struct Page;
impl View for Page {
    fn view(&self) -> El<'_> {
        row(()).gap(8.0f64)
    }
}
fn main() {}
