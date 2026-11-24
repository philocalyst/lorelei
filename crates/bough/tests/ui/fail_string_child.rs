// Strings are text leaves only: a container's child is an element (`text("..")`), not a `&str`.
use bough::prelude::*;
struct Page;
impl View for Page {
    fn view(&self) -> El<'_> {
        row(("Title", text("body")))
    }
}
fn main() {}
