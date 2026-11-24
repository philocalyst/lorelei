// A two-way port takes a writable cell: a `Reader` is read-only.
use bough::prelude::*;
struct Form { name: Reader<String> }
impl View for Form {
    fn view(&self) -> El<'_> {
        text_input(&self.name)
    }
}
fn main() {}
