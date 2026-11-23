//! Integration Card 3, workload 3: one deep heterogeneous component. Sixteen levels, each a different mix of flex,
//! text, wrapped text, handlers, hover paint, translucent paint and reactive operands, each wrapping the next.
//! What is measured is how long this takes to build (`bench/card3/compile.sh`): the first attempt at a view of
//! this depth on monomorphised layout did not finish code generation in 3 h 32 min.
//!
//! `bough::prelude` only. The leaf (`leaf`) is the function the incremental measurement edits.
use bough::builders::*;

pub struct Deep {
    pub count: Signal<u32>,
    pub fade: Signal<f32>,
    pub shift: Signal<f32>,
    pub label: String,
    pub hovers: u32,
    pub presses: u32,
}
impl Deep {
    pub fn new() -> Deep {
        Deep {
            count: Signal::new(0),
            fade: Signal::new(0.5),
            shift: Signal::new(2.0),
            label: "shared label".to_string(),
            hovers: 0,
            presses: 0,
        }
    }
    pub fn inc(&mut self) {
        *self.count += 1;
    }
    pub fn hover(&mut self, _: &bough::input::Ev) {
        self.hovers += 1;
    }
    pub fn press(&mut self, _: &bough::input::Ev) {
        self.presses += 1;
    }
}

fn leaf(v: &Deep) -> impl Element<Deep> + '_ {
    // LEAF-EDIT-MARK: the incremental build edits the literal on the next line
    row((
        text("leaf"),
        text(sig!(v: Deep => count).map(|n| format!("#{n}"))),
    ))
    .gap(4.0)
    .p(2.0)
    .bg(rgb(0xfef3c7))
}
fn level15(v: &Deep) -> impl Element<Deep> + '_ {
    column((
        each((0..3usize).map(|i| {
            row((
                block(10.0, 10.0)
                    .bg(rgb(0x10b981))
                    .translate(sig!(v: Deep => shift).map(move |s| (*s * i as f32, 0.0))),
                text(format!("item {i} of level 15")).color(rgb(0x4b5563)),
            ))
            .gap(6.0)
            .items(Align::Center)
        })),
        leaf(v),
    ))
    .gap(2.0)
    .px(4.0)
    .on_press(Deep::press)
}
fn level14(v: &Deep) -> impl Element<Deep> + '_ {
    div()
        .layout(Flex::column().gap(3.0))
        .p(3.0)
        .bg(Color::rgba(47, 107, 255, 30))
        .child(
            row((
                text("depth 14").color(rgb(0x1f2937)),
                text(v.label.as_str()).color(rgb(0x6b7280)),
            ))
            .gap(8.0)
            .justify(Justify::SpaceBetween),
        )
        .child(level15(v))
        .hover_bg(Color::rgba(47, 107, 255, 62))
        .on_hover(Deep::hover)
}
fn level13(v: &Deep) -> impl Element<Deep> + '_ {
    row((
        block(6.0, 6.0)
            .bg(rgb(0x2f6bff))
            .rounded(3.0)
            .opacity(sig!(v: Deep => fade)),
        column((
            para("A paragraph at depth 13 that wraps when the window is narrow enough to need it.")
                .font(12.0, 16.0)
                .color(rgb(0x374151)),
            level14(v),
        ))
        .gap(4.0)
        .grow(1.0),
    ))
    .gap(6.0)
    .p(3.0)
    .border(1.0, rgb(0xd1d5db))
    .rounded(6.0)
}
fn level12(v: &Deep) -> impl Element<Deep> + '_ {
    column((
        row((
            text("level 12").font(13.0, 18.0).color(rgb(0x111827)),
            text(sig!(v: Deep => count).map(|n| format!("{n} clicks")))
                .color(rgb(0x6b7280))
                .grow(1.0),
            button("+").on_click(Deep::inc),
        ))
        .items(Align::Center)
        .gap(6.0),
        level13(v),
    ))
    .gap(4.0)
    .p(4.0)
    .bg(rgb(0xcfd0d2))
    .rounded(4.0)
}
fn level11(v: &Deep) -> impl Element<Deep> + '_ {
    column((
        each((0..3usize).map(|i| {
            row((
                block(10.0, 10.0)
                    .bg(rgb(0x10b981))
                    .translate(sig!(v: Deep => shift).map(move |s| (*s * i as f32, 0.0))),
                text(format!("item {i} of level 11")).color(rgb(0x4b5563)),
            ))
            .gap(6.0)
            .items(Align::Center)
        })),
        level12(v),
    ))
    .gap(2.0)
    .px(4.0)
    .on_press(Deep::press)
}
fn level10(v: &Deep) -> impl Element<Deep> + '_ {
    div()
        .layout(Flex::column().gap(3.0))
        .p(3.0)
        .bg(Color::rgba(47, 107, 255, 26))
        .child(
            row((
                text("depth 10").color(rgb(0x1f2937)),
                text(v.label.as_str()).color(rgb(0x6b7280)),
            ))
            .gap(8.0)
            .justify(Justify::SpaceBetween),
        )
        .child(level11(v))
        .hover_bg(Color::rgba(47, 107, 255, 58))
        .on_hover(Deep::hover)
}
fn level9(v: &Deep) -> impl Element<Deep> + '_ {
    row((
        block(6.0, 6.0)
            .bg(rgb(0x2f6bff))
            .rounded(3.0)
            .opacity(sig!(v: Deep => fade)),
        column((
            para("A paragraph at depth 9 that wraps when the window is narrow enough to need it.")
                .font(12.0, 16.0)
                .color(rgb(0x374151)),
            level10(v),
        ))
        .gap(4.0)
        .grow(1.0),
    ))
    .gap(6.0)
    .p(3.0)
    .border(1.0, rgb(0xd1d5db))
    .rounded(6.0)
}
fn level8(v: &Deep) -> impl Element<Deep> + '_ {
    column((
        row((
            text("level 8").font(13.0, 18.0).color(rgb(0x111827)),
            text(sig!(v: Deep => count).map(|n| format!("{n} clicks")))
                .color(rgb(0x6b7280))
                .grow(1.0),
            button("+").on_click(Deep::inc),
        ))
        .items(Align::Center)
        .gap(6.0),
        level9(v),
    ))
    .gap(4.0)
    .p(4.0)
    .bg(rgb(0xdbdcde))
    .rounded(4.0)
}
fn level7(v: &Deep) -> impl Element<Deep> + '_ {
    column((
        each((0..3usize).map(|i| {
            row((
                block(10.0, 10.0)
                    .bg(rgb(0x10b981))
                    .translate(sig!(v: Deep => shift).map(move |s| (*s * i as f32, 0.0))),
                text(format!("item {i} of level 7")).color(rgb(0x4b5563)),
            ))
            .gap(6.0)
            .items(Align::Center)
        })),
        level8(v),
    ))
    .gap(2.0)
    .px(4.0)
    .on_press(Deep::press)
}
fn level6(v: &Deep) -> impl Element<Deep> + '_ {
    div()
        .layout(Flex::column().gap(3.0))
        .p(3.0)
        .bg(Color::rgba(47, 107, 255, 22))
        .child(
            row((
                text("depth 6").color(rgb(0x1f2937)),
                text(v.label.as_str()).color(rgb(0x6b7280)),
            ))
            .gap(8.0)
            .justify(Justify::SpaceBetween),
        )
        .child(level7(v))
        .hover_bg(Color::rgba(47, 107, 255, 54))
        .on_hover(Deep::hover)
}
fn level5(v: &Deep) -> impl Element<Deep> + '_ {
    row((
        block(6.0, 6.0)
            .bg(rgb(0x2f6bff))
            .rounded(3.0)
            .opacity(sig!(v: Deep => fade)),
        column((
            para("A paragraph at depth 5 that wraps when the window is narrow enough to need it.")
                .font(12.0, 16.0)
                .color(rgb(0x374151)),
            level6(v),
        ))
        .gap(4.0)
        .grow(1.0),
    ))
    .gap(6.0)
    .p(3.0)
    .border(1.0, rgb(0xd1d5db))
    .rounded(6.0)
}
fn level4(v: &Deep) -> impl Element<Deep> + '_ {
    column((
        row((
            text("level 4").font(13.0, 18.0).color(rgb(0x111827)),
            text(sig!(v: Deep => count).map(|n| format!("{n} clicks")))
                .color(rgb(0x6b7280))
                .grow(1.0),
            button("+").on_click(Deep::inc),
        ))
        .items(Align::Center)
        .gap(6.0),
        level5(v),
    ))
    .gap(4.0)
    .p(4.0)
    .bg(rgb(0xe7e8ea))
    .rounded(4.0)
}
fn level3(v: &Deep) -> impl Element<Deep> + '_ {
    column((
        each((0..3usize).map(|i| {
            row((
                block(10.0, 10.0)
                    .bg(rgb(0x10b981))
                    .translate(sig!(v: Deep => shift).map(move |s| (*s * i as f32, 0.0))),
                text(format!("item {i} of level 3")).color(rgb(0x4b5563)),
            ))
            .gap(6.0)
            .items(Align::Center)
        })),
        level4(v),
    ))
    .gap(2.0)
    .px(4.0)
    .on_press(Deep::press)
}
fn level2(v: &Deep) -> impl Element<Deep> + '_ {
    div()
        .layout(Flex::column().gap(3.0))
        .p(3.0)
        .bg(Color::rgba(47, 107, 255, 18))
        .child(
            row((
                text("depth 2").color(rgb(0x1f2937)),
                text(v.label.as_str()).color(rgb(0x6b7280)),
            ))
            .gap(8.0)
            .justify(Justify::SpaceBetween),
        )
        .child(level3(v))
        .hover_bg(Color::rgba(47, 107, 255, 50))
        .on_hover(Deep::hover)
}
fn level1(v: &Deep) -> impl Element<Deep> + '_ {
    row((
        block(6.0, 6.0)
            .bg(rgb(0x2f6bff))
            .rounded(3.0)
            .opacity(sig!(v: Deep => fade)),
        column((
            para("A paragraph at depth 1 that wraps when the window is narrow enough to need it.")
                .font(12.0, 16.0)
                .color(rgb(0x374151)),
            level2(v),
        ))
        .gap(4.0)
        .grow(1.0),
    ))
    .gap(6.0)
    .p(3.0)
    .border(1.0, rgb(0xd1d5db))
    .rounded(6.0)
}
fn level0(v: &Deep) -> impl Element<Deep> + '_ {
    column((
        row((
            text("level 0").font(13.0, 18.0).color(rgb(0x111827)),
            text(sig!(v: Deep => count).map(|n| format!("{n} clicks")))
                .color(rgb(0x6b7280))
                .grow(1.0),
            button("+").on_click(Deep::inc),
        ))
        .items(Align::Center)
        .gap(6.0),
        level1(v),
    ))
    .gap(4.0)
    .p(4.0)
    .bg(rgb(0xf3f4f6))
    .rounded(4.0)
}

impl View for Deep {
    fn view(&self) -> impl Element<Self> + '_ {
        div()
            .w_pct(1.0)
            .h_pct(1.0)
            .p(8.0)
            .bg(rgb(0xffffff))
            .child(level0(self))
    }
}

#[allow(dead_code)]
fn main() {
    // built and mounted headless on the lowered runtime: the fixture is a compile-time and a correctness check
    let mut app = bough::lower::App::new(
        Deep::new(),
        bough::raw::Config::new(900.0, 1400.0, 1.0, bough::ui::default_font()),
    );
    app.check().expect("the deep view holds the four oracles");
    let (records, _, _) = app.world_stats();
    println!(
        "deep view: {} records, {} describes, legacy passes {}",
        app.record_count(),
        app.stats.describes,
        bough::lower::provenance::counts().legacy()
    );
    let _ = records;
}
