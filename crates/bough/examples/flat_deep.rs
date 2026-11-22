//! The deep heterogeneous view (`examples/deep_view.rs`: sixteen levels, each a different mix of flex, text,
//! handlers, hover paint, translucent paint and reactive operands, each wrapping the next) written against the
//! flat core's bindings (`bough::rethink::ui`). Its purpose is the same as the original's: to be built. Every
//! level here returns the one element type, so there is nothing for the compiler to monomorphise per level.
//!
//! Differences from the original, by contract of `rethink::ui`: the built-in face instead of shaped text, and
//! the paragraphs are single lines (no wrapping); hover handlers are hover paints only.
//!
//! The leaf (`leaf`) is the function the incremental measurement edits.
use bough::layout::{Align, Justify, Styled};
use bough::rethink::flat::Slot;
use bough::rethink::ui::*;

#[derive(Clone, Copy)]
pub struct Deep {
    pub count: Slot<u32>,
    /// 0 ..= 255.
    pub fade: Slot<u32>,
    /// Device pixels, Q0.
    pub shift: Slot<i32>,
    pub presses: Slot<u32>,
}

fn rgb(c: u32) -> u32 {
    // 0xRRGGBB as the core's straight-alpha colour (red low)
    0xff00_0000 | (c & 0xff) << 16 | (c & 0xff00) | (c >> 16 & 0xff)
}
fn rgba(r: u32, g: u32, b: u32, a: u32) -> u32 {
    a << 24 | b << 16 | g << 8 | r
}

fn leaf(v: Deep) -> El {
    // LEAF-EDIT-MARK: the incremental build edits the literal on the next line
    row().gap(4.0).p(2.0).bg(rgb(0xfef3c7)).child(text("leaf")).child(num("#", v.count, "", 6))
}
fn items(v: Deep, level: usize, inner: El) -> El {
    column()
        .gap(2.0)
        .px(4.0)
        .on_click(move |s| {
            let n = s.get(v.presses);
            s.set(v.presses, n + 1);
        })
        .children((0..3).map(|i| {
            row()
                .gap(6.0)
                .items(Align::Center)
                .child(div().size(10.0, 10.0).bg(rgb(0x10b981)).translate_x_scaled(v.shift, i * 256))
                .child(text(format!("item {i} of level {level}")).color(rgb(0x4b5563)))
        }))
        .child(inner)
}
fn tinted(v: Deep, level: usize, alpha: u32, inner: El) -> El {
    let _ = v;
    column()
        .gap(3.0)
        .p(3.0)
        .bg(rgba(47, 107, 255, alpha))
        .hover_bg(rgba(47, 107, 255, alpha + 32))
        .child(row().gap(8.0).justify(Justify::SpaceBetween).child(text(format!("depth {level}")).color(rgb(0x1f2937))).child(text("shared label").color(rgb(0x6b7280))))
        .child(inner)
}
fn bordered(v: Deep, level: usize, inner: El) -> El {
    row()
        .gap(6.0)
        .p(3.0)
        .border(1.0, rgb(0xd1d5db))
        .rounded(6.0)
        .child(div().size(6.0, 6.0).bg(rgb(0x2f6bff)).rounded(3.0).opacity(v.fade))
        .child(column().gap(4.0).grow(1.0).child(text(format!("A paragraph at depth {level} that would wrap in the original.")).color(rgb(0x374151))).child(inner))
}
fn headed(v: Deep, level: usize, bg: u32, inner: El) -> El {
    column()
        .gap(4.0)
        .p(4.0)
        .bg(rgb(bg))
        .rounded(4.0)
        .child(
            row()
                .items(Align::Center)
                .gap(6.0)
                .child(text(format!("level {level}")).color(rgb(0x111827)))
                .child(num("", v.count, " clicks", 6).color(rgb(0x6b7280)).grow(1.0))
                .child(text("+").p(4.0).bg(rgb(0xe5e7eb)).hover_bg(rgb(0xdbeafe)).on_click(move |s| {
                    let n = s.get(v.count);
                    s.set(v.count, n + 1);
                })),
        )
        .child(inner)
}
fn level15(v: Deep) -> El {
    items(v, 15, leaf(v))
}
fn level14(v: Deep) -> El {
    tinted(v, 14, 30, level15(v))
}
fn level13(v: Deep) -> El {
    bordered(v, 13, level14(v))
}
fn level12(v: Deep) -> El {
    headed(v, 12, 0xcfd0d2, level13(v))
}
fn level11(v: Deep) -> El {
    items(v, 11, level12(v))
}
fn level10(v: Deep) -> El {
    tinted(v, 10, 26, level11(v))
}
fn level9(v: Deep) -> El {
    bordered(v, 9, level10(v))
}
fn level8(v: Deep) -> El {
    headed(v, 8, 0xdbdcde, level9(v))
}
fn level7(v: Deep) -> El {
    items(v, 7, level8(v))
}
fn level6(v: Deep) -> El {
    tinted(v, 6, 22, level7(v))
}
fn level5(v: Deep) -> El {
    bordered(v, 5, level6(v))
}
fn level4(v: Deep) -> El {
    headed(v, 4, 0xe7e8ea, level5(v))
}
fn level3(v: Deep) -> El {
    items(v, 3, level4(v))
}
fn level2(v: Deep) -> El {
    tinted(v, 2, 18, level3(v))
}
fn level1(v: Deep) -> El {
    bordered(v, 1, level2(v))
}
fn level0(v: Deep) -> El {
    headed(v, 0, 0xf3f4f6, level1(v))
}

pub fn mount(w: i32, h: i32) -> (FlatUi, Deep) {
    let mut ui = FlatUi::new(w, h, 1.0);
    let wd = &mut ui.world;
    let v = Deep { count: wd.slot(0u32), fade: wd.slot(128u32), shift: wd.slot(2i32), presses: wd.slot(0u32) };
    ui.mount(div().w_pct(1.0).h_pct(1.0).p(8.0).bg(rgb(0xffffff)).child(level0(v)));
    (ui, v)
}

#[allow(dead_code)]
fn main() {
    let (mut ui, v) = mount(900, 1400);
    ui.check().expect("the flat deep view holds its oracles");
    let instr = bough::sys::perf::thread_instructions;
    let ot = bough::sys::perf::tinstr_overhead();
    let mut cost = |f: &mut dyn FnMut(&mut FlatUi, u32)| {
        let mut t: Vec<(u64, u64)> = (0..41)
            .map(|k| {
                let t0 = instr();
                f(&mut ui, k);
                ui.world.settle();
                let t1 = instr();
                ui.frame();
                ((t1 - t0).saturating_sub(ot), (instr() - t0).saturating_sub(2 * ot))
            })
            .collect();
        t.sort();
        t[20]
    };
    let count = cost(&mut |ui, k| {
        ui.world.set(v.count, k);
    });
    let fade = cost(&mut |ui, k| {
        ui.world.set(v.fade, 100 + k % 100);
    });
    let shift = cost(&mut |ui, k| {
        ui.world.set(v.shift, (k % 9) as i32);
    });
    let hover = cost(&mut |ui, k| ui.pointer_move(300, 200 + (k % 2) as i32 * 400));
    ui.check().expect("still");
    println!(
        "flat deep view: {} records, {} state words; thread instructions (turn, turn+frame): count {count:?}, fade {fade:?}, shift {shift:?}, hover {hover:?}",
        ui.world.records(),
        ui.world.state.len()
    );
}
