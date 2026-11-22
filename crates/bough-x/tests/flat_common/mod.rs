//! The scene the flat core's gates share: a themed window with a scrolled, clipped list of hoverable rows, a
//! selection, a counter, a card with stacked shadows that moves, two overlapping glass panels, a fixed header
//! over the list and an opaque cover over part of everything.
#![allow(dead_code)]
use bough_x::flat::store::ROOT;
use bough_x::flat::*;

pub const W: i32 = 640;
pub const H: i32 = 480;
pub const ROW_H: i32 = 28;
pub const LIST: Rect = Rect::new(16, 72, 336, 400);

// palette slots
pub const P_BG: u16 = 0;
pub const P_PANEL: u16 = 1;
pub const P_ROW: u16 = 2;
pub const P_INK: u16 = 3;
pub const P_ACCENT: u16 = 4;

pub struct Scene {
    pub w: FlatWorld,
    pub count: Slot<u32>,
    pub selected: Slot<u32>,
    pub scroll: Slot<i32>,
    pub clip: Slot<bool>,
    pub card_x: Slot<i32>,
    pub shadow_blur: Slot<u32>,
    pub glass_radius: Slot<u32>,
    pub cover_on: Slot<u32>,
    pub hidden: Slot<u32>,
    pub buried: Slot<u32>,
    pub list: SpaceId,
    pub rows: Vec<(RecId, RecId)>,
    pub card: RecId,
    pub cover: RecId,
    pub header: RecId,
    pub glass: RecId,
    pub next_row: u32,
    pub fade: Val,
}

pub fn lcg(s: &mut u64) -> u32 {
    *s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    (*s >> 33) as u32
}

impl Scene {
    pub fn new(rows: u32) -> Scene {
        Scene::sized(rows, W, H)
    }
    pub fn sized(rows: u32, w: i32, h: i32) -> Scene {
        let mut w_ = FlatWorld::new(w, h);
        let w = &mut w_;
        w.set_palette(
            5,
            &[
                0xfff6_f4f3, 0xffff_ffff, 0xffee_e8e0, 0xff20_1810, 0xffff_6b2f, // light
                0xff18_1614, 0xff2a_2622, 0xff3a_342e, 0xfff0_e8e0, 0xffff_9a5a, // dark
            ],
        );
        let count = w.slot(0u32);
        let selected = w.slot(u32::MAX);
        let scroll = w.slot(0i32);
        let clip = w.slot(true);
        let card_x = w.slot(380i32);
        let shadow_blur = w.slot(10u32);
        let glass_radius = w.slot(8u32);
        let cover_on = w.slot(255u32);
        let hidden = w.slot(0xff00_00ffu32);
        let buried = w.slot(0xff00_ff00u32);
        // the window, then a panel the list sits in
        w.add_record(RecordDesc::rect(ROOT, Rect::new(0, 0, w.size().0, w.size().1), w.pal(P_BG)));
        // something that will be hidden under the cover and beside the glass: its colour is a state word
        w.add_record(RecordDesc::rect(ROOT, Rect::new(470, 258, 520, 286), hidden));
        // and something buried under the list's opaque panel, below the glass that overlaps the panel
        w.add_record(RecordDesc::rect(ROOT, Rect::new(296, 150, 316, 190), buried));
        let port = w.add_record(RecordDesc::rect(ROOT, LIST, w.pal(P_PANEL)));
        let list = w.add_space(ROOT, Some(port), clip, 0, scroll);
        let mut s = Scene {
            w: w_,
            count,
            selected,
            scroll,
            clip,
            card_x,
            shadow_blur,
            glass_radius,
            cover_on,
            hidden,
            buried,
            list,
            rows: Vec::new(),
            card: RecId { index: 0, generation: 0 },
            cover: RecId { index: 0, generation: 0 },
            header: RecId { index: 0, generation: 0 },
            glass: RecId { index: 0, generation: 0 },
            next_row: 0,
            fade: Val::Const(0),
        };
        for _ in 0..rows {
            s.push_row();
        }
        let w = &mut s.w;
        // a card with two stacked shadows whose blur is a state word; it moves with `card_x`
        let fx = Effect::new().shadow(0, 0x5000_0000u32, 0, 6, shadow_blur, 2).shadow(1, 0x3020_10ffu32, -3, -2, 4, 0).gradient(1, 0xffd0_e0ffu32);
        let mut card = RecordDesc::rect(ROOT, Rect::new(0, 130, 120, 210), 0xffff_ffffu32).radius_px(10).effect(fx);
        card.x = card_x.into();
        s.card = w.add_record(card);
        // a counter on the card: follows it through an expression
        let mut b = w.tape.build();
        let (x, off) = (b.slot(card_x), b.konst(12));
        let lx = b.add(x, off);
        let lx = b.finish(lx);
        let mut label = RecordDesc::label(ROOT, Rect::new(0, 150, 96, 166), TextSrc::Num { prefix: "N ".into(), slot: count.id(), suffix: String::new() }, 2, w.pal(P_INK));
        label.x = lx;
        w.add_record(label);
        // glass over the right part of the list and the card's lane; its radius is a state word
        s.glass = w.add_record(
            RecordDesc::rect(ROOT, Rect::new(300, 100, 470, 260), 0x40ff_ffffu32)
                .radius_px(12)
                .border_px(1, 0x80ff_ffffu32)
                .effect(Effect::new().backdrop_blur(glass_radius)),
        );
        // a second sheet of glass over a corner of the first: its backdrop is the first's output
        w.add_record(RecordDesc::rect(ROOT, Rect::new(430, 220, 560, 330), 0x30a0_c0ffu32).radius_px(6).effect(Effect::new().backdrop_blur(4)));
        // a fixed header over the top of the list, translucent, with a hover site of its own depth
        let site = w.add_site(0);
        let tint = w.hover_sel(site, 0xc0ff_ffff, 0xc0ff_e0c0);
        s.fade = w.add_anim(tint, 120, true, false);
        s.header = w.add_record(RecordDesc { fill: s.fade, ..RecordDesc::rect(ROOT, Rect::new(0, 40, w.size().0, 96), 0u32).hit(site, 0) });
        // an opaque cover over the hidden box and a strip of the list; it can be made translucent
        s.cover = w.add_record(RecordDesc::rect(ROOT, Rect::new(180, 240, 420, 380), 0xff40_5060u32).opacity(cover_on));
        w.add_record(RecordDesc::rect(ROOT, Rect::new(448, 236, 548, 300), 0xff70_3050u32).opacity(cover_on));
        s
    }
    /// One more row at the end of the list: a hoverable, selectable box and its label.
    pub fn push_row(&mut self) {
        let k = self.next_row;
        self.next_row += 1;
        let y = self.rows.len() as i32 * ROW_H;
        let ids = self.row_at(k, y, None);
        self.rows.push(ids);
    }
    fn row_at(&mut self, k: u32, y: i32, before: Option<RecId>) -> (RecId, RecId) {
        let w = &mut self.w;
        let site = w.add_site(1);
        // selected ? accent : (hovered ? hover : palette row)
        let theme = w.theme_slot();
        let mut b = w.tape.build();
        let (sel, me) = (b.slot(self.selected), b.konst(k));
        let is = b.eq(sel, me);
        let (acc, row) = (b.palette(P_ACCENT, theme), b.palette(P_ROW, theme));
        let hovering = b.hovered(site);
        let hc = b.konst(0xffc8_d8f0u32);
        let base = b.select(hovering, hc, row);
        let out = b.select(is, acc, base);
        let fill = b.finish(out);
        let mut bg = RecordDesc::rect(self.list, Rect::new(4, y + 2, 316, y + ROW_H - 2), fill).hit(site, k + 1).key(k as u64 + 1);
        if k % 5 == 3 {
            bg = bg.radius_px(6).effect(Effect::new().shadow(0, 0x4000_0000u32, 0, 2, 5, 0));
        }
        let label = RecordDesc::label(
            self.list,
            Rect::new(12, y + 9, 300, y + 21),
            TextSrc::Num { prefix: format!("ROW {k} #"), slot: self.count.id(), suffix: String::new() },
            1,
            w.pal(P_INK),
        );
        match before {
            Some(a) => (w.insert_before(a, bg), w.insert_before(a, label)),
            None => {
                // rows are painted under everything described after the list: keep them under the first overlay
                match self.rows.is_empty() {
                    true => (w.add_record(bg), w.add_record(label)),
                    false if self.card.index == 0 && self.card.generation == 0 && self.glass.index == 0 => (w.add_record(bg), w.add_record(label)),
                    false => (w.insert_before(self.card, bg), w.insert_before(self.card, label)),
                }
            }
        }
    }
    /// Remove row `i` (by position) and close the gap by rebuilding the rows after it one row up.
    pub fn remove_row(&mut self, i: usize) {
        let tail: Vec<(RecId, RecId)> = self.rows.drain(i..).collect();
        for (a, b) in &tail {
            assert!(self.w.remove(*a) && self.w.remove(*b));
        }
        // the rows after it are described again at their new places (their keys say they are the same rows)
        let keys: Vec<u32> = (0..tail.len() as u32 - 1).map(|_| {
            let k = self.next_row;
            self.next_row += 1;
            k
        }).collect();
        for k in keys {
            let y = self.rows.len() as i32 * ROW_H;
            let ids = self.row_at(k, y, Some(self.card));
            self.rows.push(ids);
        }
    }
    pub fn row_center(&self, i: usize) -> (i32, i32) {
        let r = self.w.screen_rect(self.rows[i].0).unwrap();
        ((r.x0 + r.x1) / 2, (r.y0 + r.y1) / 2)
    }
    /// One random operation; returns what it was.
    pub fn step(&mut self, seed: &mut u64) -> &'static str {
        let r = lcg(seed);
        let n = self.rows.len() as u32;
        match r % 14 {
            0 | 1 => {
                self.w.pointer_move((lcg(seed) % W as u32) as i32, (lcg(seed) % H as u32) as i32);
                "pointer"
            }
            2 | 3 => {
                let max = (n as i32 * ROW_H - (LIST.y1 - LIST.y0)).max(0) + 40;
                let d = (lcg(seed) % 90) as i32 - 45;
                let at = (self.w.get(self.scroll) + d).clamp(-20, max);
                self.w.set(self.scroll, at);
                "scroll"
            }
            4 => {
                self.w.set(self.count, lcg(seed) % 1000);
                "count"
            }
            5 => {
                self.w.set(self.selected, lcg(seed) % (self.next_row + 2));
                "select"
            }
            6 => {
                let on = self.w.get(self.clip);
                self.w.set(self.clip, !on);
                "clip_overflow"
            }
            7 => {
                let t = self.w.get(self.w.theme_slot());
                self.w.set_theme(1 - t);
                "theme"
            }
            8 => {
                self.w.set(self.card_x, 150 + (lcg(seed) % 420) as i32);
                "move card"
            }
            9 => {
                self.w.set(self.shadow_blur, lcg(seed) % 24);
                "shadow blur"
            }
            10 => {
                self.w.set(self.glass_radius, [0, 3, 8, 14][(lcg(seed) % 4) as usize]);
                "glass radius"
            }
            11 => {
                self.w.set(self.hidden, 0xff00_0000 | lcg(seed));
                self.w.set(self.buried, 0xff00_0000 | lcg(seed));
                let c = self.w.get(self.cover_on);
                if lcg(seed) % 4 == 0 {
                    self.w.set(self.cover_on, if c == 255 { 140 } else { 255 });
                }
                "hidden / cover"
            }
            12 => {
                if n > 3 && lcg(seed) % 2 == 0 {
                    self.remove_row((lcg(seed) % n) as usize);
                    "remove row"
                } else {
                    self.push_row();
                    "insert row"
                }
            }
            _ => {
                let t = self.w.get(self.w.time_slot());
                self.w.set_time(t + 1 + lcg(seed) % 90);
                "time"
            }
        }
    }
}
