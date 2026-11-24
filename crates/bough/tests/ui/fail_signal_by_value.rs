// A property takes a feed: a signal is passed by reference (`&self.tint`), not by value.
use bough::prelude::*;
struct Card { tint: Signal<Color> }
impl View for Card {
    fn view(&self) -> El<'_> {
        div().bg(self.tint)
    }
}
fn main() {}
