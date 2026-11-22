//! Integration Card 3's vertical-slice application, written against `bough::prelude` only: two signals, opaque
//! state (a theme the view reads as a plain field), a header with buttons, a keyed scrollable list of row views
//! with ASCII and non-Latin text and emoji, hover paints, a key handler, a translucent overlay.
//!
//! The same source runs on the legacy runtime (`bough::raw::App`) and on the lowered one (`bough::lower::App`):
//! the tests (`tests/lowered_slice.rs`) and the benchmark (`examples/card3_cost.rs`) include this file.
use bough::prelude::*;

pub const ROW_H: f32 = 28.0;
pub const LABELS: [&str; 8] = [
    "plain ASCII row",
    "naïve café, Ångström",
    "日本語のテキスト",
    "emoji 😀 🎉 row",
    "Ελληνικά και кириллица",
    "mixed 中文 and Latin",
    "numbers 1234567890",
    "مرحبا بالعالم",
];

pub struct Item {
    pub id: u32,
    pub label: String,
    pub done: bool,
}
impl View for Item {
    fn view(&self) -> impl Element<Self> + '_ {
        row((
            text(self.id).color(rgb(0x6b7280)).w(56.0),
            text(self.label.as_str()).color(rgb(0x111827)).grow(1.0),
            text(if self.done { "done" } else { "open" }).color(if self.done {
                rgb(0x15803d)
            } else {
                rgb(0x9ca3af)
            }),
        ))
        .items(Align::Center)
        .gap(8.0)
        .px(12.0)
        .h(ROW_H)
        .bg(if self.id % 2 == 0 {
            rgb(0xf9fafb)
        } else {
            rgb(0xffffff)
        })
        .hover_bg(rgb(0xdbeafe))
    }
}

pub struct Slice {
    pub title: Signal<String>,
    pub count: Signal<u32>,
    /// Opaque state: read by `view` as a plain field.
    pub dark: bool,
    pub scroll: Signal<f32>,
    pub items: List<Item>,
    pub next_id: u32,
    pub keys: u32,
}

impl Slice {
    pub fn new(n: usize) -> Slice {
        let mut s = Slice {
            title: Signal::new("Vertical slice".to_string()),
            count: Signal::new(0),
            dark: false,
            scroll: Signal::new(0.0),
            items: List::new(),
            next_id: 0,
            keys: 0,
        };
        for _ in 0..n {
            s.add();
        }
        *s.count = 0;
        s
    }
    fn item(&mut self) -> Item {
        let id = self.next_id;
        self.next_id += 1;
        Item {
            id,
            label: LABELS[id as usize % LABELS.len()].to_string(),
            done: id % 3 == 0,
        }
    }
    pub fn add(&mut self) {
        let it = self.item();
        self.items.push(it);
        *self.count += 1;
    }
    pub fn add_front(&mut self) {
        let it = self.item();
        self.items.insert(0, it);
        *self.count += 1;
    }
    pub fn remove_first(&mut self) {
        if !self.items.is_empty() {
            self.items.remove(0);
            *self.count += 1;
        }
    }
    pub fn reverse(&mut self) {
        self.items.reverse();
        *self.count += 1;
    }
    pub fn toggle_theme(&mut self) {
        self.dark = !self.dark;
    }
    pub fn key(&mut self, e: &bough::input::Ev) {
        self.keys += 1;
        let step = match e.key {
            Some(Key::Down) => ROW_H,
            Some(Key::Up) => -ROW_H,
            Some(Key::PageDown) => 10.0 * ROW_H,
            Some(Key::PageUp) => -10.0 * ROW_H,
            _ => return,
        };
        let max = (self.items.len() as f32 * ROW_H - ROW_H).max(0.0);
        let to = (*self.scroll + step).clamp(0.0, max);
        self.scroll.set(to);
    }
}

impl View for Slice {
    fn view(&self) -> impl Element<Self> + '_ {
        let (bg, bar, ink) = if self.dark {
            (rgb(0x111827), rgb(0x1f2937), rgb(0xf9fafb))
        } else {
            (rgb(0xe5e7eb), rgb(0xffffff), rgb(0x111827))
        };
        column((
            row((
                text(sig!(self.title)).font(16.0, 24.0).color(ink),
                text(sig!(self.count).map(|n| format!("{n} edits")))
                    .color(rgb(0x6b7280))
                    .grow(1.0),
                button("Add").on_click(Slice::add),
                button("Add first").on_click(Slice::add_front),
                button("Remove").on_click(Slice::remove_first),
                button("Reverse").on_click(Slice::reverse),
                button("Theme").on_click(Slice::toggle_theme),
            ))
            .items(Align::Center)
            .gap(8.0)
            .px(12.0)
            .h(48.0)
            .bg(bar),
            // a translucent band over the top of the list: what is under it must show through
            div()
                .relative()
                .grow(1.0)
                .child(
                    list(sig!(self.items))
                        .scroll(sig!(self.scroll))
                        .estimate(ROW_H)
                        .w_pct(1.0)
                        .h_pct(1.0)
                        .bg(rgb(0xffffff))
                        .on_key(Slice::key),
                )
                .child(
                    block(160.0, 40.0)
                        .abs(24.0, 12.0)
                        .bg(Color::rgba(47, 107, 255, 96))
                        .rounded(8.0),
                ),
        ))
        .w_pct(1.0)
        .h_pct(1.0)
        .bg(bg)
    }
}

#[allow(dead_code)]
fn main() {
    Application::run(
        Window::document(Slice::new(1000))
            .title("Slice")
            .size(640.0, 480.0),
    );
}
