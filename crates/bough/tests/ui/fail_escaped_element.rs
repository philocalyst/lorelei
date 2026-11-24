// An element borrows its description: it cannot outlive the data it borrows.
use bough::prelude::*;
struct Page;
impl View for Page {
    fn view(&self) -> El<'_> {
        let title = String::from("hi");
        text(&title)
    }
}
fn main() {}
