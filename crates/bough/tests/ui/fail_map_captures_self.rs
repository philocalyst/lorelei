// A mapped source's closure cannot borrow the view: what it depends on belongs in a signal.
use bough::prelude::*;
struct Gauge { level: Signal<f32>, scale: f32 }
impl View for Gauge {
    fn view(&self) -> El<'_> {
        div().w(self.level.map(|l| l * self.scale))
    }
}
fn main() {}
