// A route is a method of the view (`fn(&mut Self)`, `fn(&mut Self, &Ev)`) or one bound with `.with(value)`.
use bough::prelude::*;
struct Counter { n: u32 }
impl Counter {
    fn add(&mut self, k: u32) { self.n += k; }
}
impl View for Counter {
    fn view(&self) -> El<'_> {
        button("+").on_click(Counter::add)
    }
}
fn main() {}
