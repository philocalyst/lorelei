// Pass: an integer signal field is a text port (the zero-rerun spelling of a counter's label).
use bough::prelude::*;
struct Counter { count: Signal<u32> }
impl View for Counter {
    fn view(&self) -> El<'_> {
        text(&self.count)
    }
}
fn main() {}
