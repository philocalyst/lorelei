//! Progressive enhancement (`designs/reactivity/final/examples/progressive.rs`, 00 §2.2): one counter, promoted a
//! rung at a time, with the same `view` text.
//!
//!   rung 0  `count: u32`          immediate: a click reruns `view`; the re-description patches the label
//!   rung 1  `count: Signal<u32>`  retained: `self.count.map(..)` is a port; a click patches the label and
//!                                 nothing reruns
//!   rung 2  `rows: Vec<Row>`      a sequence: `column(self.rows.iter().map(Row::view))`
//!
//!   cargo run --release -p bough --example progressive            # a window: rung 1
//!   cargo test --release -p bough --test el_surface               # the rungs' pixels compared
use bough::prelude::*;

const BAR: Flex = Flex::row().align(Align::Center).gap(12.0);

pub mod rung0 {
    use super::*;
    pub struct Counter {
        pub count: u32,
    }
    impl Counter {
        pub fn inc(&mut self) {
            self.count += 1; // a plain field: the turn re-describes the view
        }
    }
    impl View for Counter {
        fn view(&self) -> El<'_> {
            div().layout(BAR).p(24).child((text(format!("Count: {}", self.count)), button("+").on_click(Counter::inc)))
        }
    }
}

pub mod rung1 {
    use super::*;
    pub struct Counter {
        pub count: Signal<u32>,
    }
    impl Counter {
        pub fn inc(&mut self) {
            *self.count += 1; // a publish: the label's port patches, `view` does not rerun
        }
    }
    impl View for Counter {
        fn view(&self) -> El<'_> {
            div().layout(BAR).p(24).child((text(self.count.map(|n| format!("Count: {n}"))), button("+").on_click(Counter::inc)))
        }
    }
}

pub mod rung2 {
    use super::*;
    pub struct Row {
        pub label: String,
        pub count: u32,
    }
    pub struct Board {
        pub rows: Vec<Row>,
    }
    impl Board {
        pub fn add(&mut self) {
            let n = self.rows.len();
            self.rows.push(Row { label: format!("row {n}"), count: 0 });
        }
        pub fn bump(&mut self, i: usize) {
            self.rows[i].count += 1;
        }
    }
    fn row_view(i: usize, r: &Row) -> El<'_> {
        row((text(r.label.as_str()).w(80), text(r.count), button("+").on_click(Board::bump.with(i)))).gap(8)
    }
    impl View for Board {
        fn view(&self) -> El<'_> {
            column((column(self.rows.iter().enumerate().map(|(i, r)| row_view(i, r))).gap(4), button("Add row").on_click(Board::add))).gap(8).p(16)
        }
    }
}

#[allow(dead_code)]
fn main() {
    Application::run(Window::document(rung1::Counter { count: Signal::new(0) }).title("Counter"));
}
