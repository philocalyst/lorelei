// A port takes its own property type: a width is a length, not a colour.
use bough::prelude::*;
struct Page { tint: Signal<Color> }
impl View for Page {
    fn view(&self) -> El<'_> {
        div().w(&self.tint)
    }
}
fn main() {}
