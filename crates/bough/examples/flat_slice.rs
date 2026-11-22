//! The vertical slice (`examples/slice.rs`) written against the flat core's bindings (`bough::flat::ui`):
//! a title and a counter that are state words, a themed header with buttons, a scrolled list of hoverable
//! rows, a translucent overlay. No view type, no generic element: every row is the same plain data.
//!
//! What it does not have that the prelude slice has: shaped text (the built-in face stands in), rows inserted
//! at run time (layout is computed at mount; `Add` appends nothing here and only counts), a resize that
//! re-flows. `examples/flat_cost.rs` states this next to every number it prints for this file.
use bough::layout::{Align, Styled};
use bough::flat::{Slot, StrSlot, Val};
use bough::flat::ui::*;

pub const ROW_H: f32 = 28.0;
pub const P_BG: u16 = 0;
pub const P_BAR: u16 = 1;
pub const P_INK: u16 = 2;
pub const P_MUTED: u16 = 3;
pub const P_ROW_A: u16 = 4;
pub const P_ROW_B: u16 = 5;

pub struct FlatSlice {
    pub ui: FlatUi,
    pub title: StrSlot,
    pub count: Slot<u32>,
    pub scroll: Slot<i32>,
}

fn button(name: &str, count: Slot<u32>) -> El {
    text(name).text_scale(2).color(0xff11_1827u32).p(4.0).bg(0xffe5_e7ebu32).hover_bg(0xffdb_eafe).on_click(move |s| {
        let n = s.get(count);
        s.set(count, n + 1);
    })
}

impl FlatSlice {
    pub fn new(n: usize, w: i32, h: i32, scale: f32) -> FlatSlice {
        let mut ui = FlatUi::new(w, h, scale);
        let wd = &mut ui.world;
        wd.set_palette(
            6,
            &[
                0xffeb_e7e5, 0xffff_ffff, 0xff27_1811, 0xff80_726b, 0xfffb_faf9, 0xffff_ffff, // light
                0xff27_1811, 0xff37_291f, 0xfffb_faf9, 0xffaf_a39c, 0xff30_241c, 0xff37_291f, // dark
            ],
        );
        let title = wd.state.alloc_str("Vertical slice");
        let (count, scroll) = (wd.slot(0u32), wd.slot(0i32));
        let pal = |k: u16| -> Val { wd.pal(k) };
        let theme = wd.theme_slot();
        let rows = list(n, |i| {
            row()
                .items(Align::Center)
                .gap(8.0)
                .px(12.0)
                .h(ROW_H)
                .bg(pal(if i % 2 == 0 { P_ROW_A } else { P_ROW_B }))
                .hover_bg(0xfffe_eadb)
                .child(text(i.to_string()).color(pal(P_MUTED)).w(56.0))
                .child(text(["plain ascii row", "numbers 1234567890", "mixed row of text"][i % 3]).color(pal(P_INK)).grow(1.0))
                .child(text(if i % 3 == 0 { "done" } else { "open" }).color(if i % 3 == 0 { 0xff3d_8015u32 } else { 0xffaf_a39c }))
        });
        let root = column()
            .w_pct(1.0)
            .h_pct(1.0)
            .bg(pal(P_BG))
            .child(
                row()
                    .items(Align::Center)
                    .gap(8.0)
                    .px(12.0)
                    .h(48.0)
                    .bg(pal(P_BAR))
                    .child(label(title, 24).text_scale(2).color(pal(P_INK)))
                    .child(num("", count, " edits", 6).color(pal(P_MUTED)).grow(1.0))
                    .child(button("Add", count))
                    .child(button("Remove", count))
                    .child(text("Theme").text_scale(2).color(0xff11_1827u32).p(4.0).bg(0xffe5_e7ebu32).hover_bg(0xffdb_eafe).on_click(move |s| {
                        let t = s.get(theme);
                        s.set(theme, 1 - t);
                    })),
            )
            .child(
                div()
                    .relative()
                    .grow(1.0)
                    .child(column().w_pct(1.0).h_pct(1.0).bg(pal(P_ROW_B)).scroll(0, scroll).child(rows))
                    .child(div().size(160.0, 40.0).abs(24.0, 12.0).bg(0x60ff_6b2fu32).rounded(8.0)),
            );
        ui.mount(root);
        FlatSlice { ui, title, count, scroll }
    }
}

#[allow(dead_code)]
fn main() {
    let mut s = FlatSlice::new(1000, 1280, 960, 2.0);
    s.ui.check().expect("the flat slice holds its oracles");
    println!("flat slice: {} records; mount instructions [tree, layout, describe (with the list's own layout), commit]: {:?}", s.ui.world.records(), s.ui.mount_cost);
}
