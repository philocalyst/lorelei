//! A description written against the flat core's own bindings: no view type, no generic element, no
//! re-description. An [`El`] is one plain struct; its layout properties are the façade's ([`Styled`]), laid out
//! once by `bough-lay`; everything that can change afterwards is a [`Val`] (a state word, a palette entry, an
//! expression, an animation), so a turn costs the bindings that read what changed.
//!
//! ```ignore
//! let mut ui = FlatUi::new(640, 480, 1.0);
//! let count = ui.world.slot(0u32);
//! ui.mount(
//!     column().p(8.0).gap(4.0).bg(0xffff_ffffu32)
//!         .child(num("clicks ", count, "", 4))
//!         .child(text("+").bg(0xffe0_e0e0u32).hover_bg(0xffc0_d0ff).on_click(move |s| { let n = s.get(count); s.set(count, n + 1); })),
//! );
//! ui.frame();
//! ```
//!
//! Contract: text is the core's built-in 5 x 7 face (one line, no wrapping); layout is computed at mount (a
//! binding moves or recolours a box, it does not re-flow its siblings); lengths in [`Styled`] are points.
use crate::layout::{Spec, Styled};
use bough_lay::heap::Tree;
use bough_flat::raster::cell;
use bough_flat::store::ROOT;
use bough_flat::{Effect, FlatWorld, FrameInfo, Kind, NoExt, Rect, RecordDesc, Slot, SpaceId, StateBlock, TextSrc, Val};

/// What a click runs: it writes state words; the next frame shows it.
pub type Action = Box<dyn FnMut(&mut StateBlock)>;

/// One element. Build with [`div`], [`row`], [`column`], [`text`], [`num`]; size and place with [`Styled`].
pub struct El {
    spec: Spec,
    fill: Val,
    hover: Option<u32>,
    transition_ms: u32,
    border: (f32, Val),
    radius: f32,
    opacity: Val,
    effect: Option<Effect>,
    text: Option<(TextSrc, usize)>,
    text_scale: u32,
    kids: Vec<El>,
    scroll: Option<(Val, Val)>,
    clip: Option<Val>,
    click: Option<Action>,
    shift: Option<(Val, Val)>,
    shift_scaled: Option<(Slot<i32>, i32)>,
    solid: bool,
}

impl Styled for El {
    fn spec_mut(&mut self) -> &mut Spec {
        &mut self.spec
    }
}

fn el(spec: Spec) -> El {
    El {
        spec,
        fill: Val::Const(0),
        hover: None,
        transition_ms: 0,
        border: (0.0, Val::Const(0)),
        radius: 0.0,
        opacity: Val::Const(255),
        effect: None,
        text: None,
        text_scale: 1,
        kids: Vec::new(),
        scroll: None,
        clip: None,
        click: None,
        shift: None,
        shift_scaled: None,
        solid: false,
    }
}
/// A box whose children are laid out in a row.
pub fn div() -> El {
    el(Spec::new(true, false))
}
pub fn row() -> El {
    el(Spec::new(true, false))
}
pub fn column() -> El {
    el(Spec::new(true, true))
}
/// A line of constant text. `.bg(..)` is its ink.
pub fn text(s: impl Into<String>) -> El {
    let s = s.into();
    let n = s.len();
    El { text: Some((TextSrc::Static(s), n)), fill: Val::Const(0xff00_0000), ..el(Spec::new(false, false)) }
}
/// `prefix`, a state word in decimal, `suffix`; laid out for up to `digits` digits.
pub fn num(prefix: &str, slot: Slot<u32>, suffix: &str, digits: usize) -> El {
    let n = prefix.len() + digits + suffix.len();
    El { text: Some((TextSrc::Num { prefix: prefix.into(), slot: slot.id(), suffix: suffix.into() }, n)), fill: Val::Const(0xff00_0000), ..el(Spec::new(false, false)) }
}
/// A string of the state block's side table; laid out for up to `chars` characters.
pub fn label(s: bough_flat::StrSlot, chars: usize) -> El {
    El { text: Some((TextSrc::Str(s), chars)), fill: Val::Const(0xff00_0000), ..el(Spec::new(false, false)) }
}
/// `n` children in a column, child `i` from `f(i)`.
pub fn list(n: usize, mut f: impl FnMut(usize) -> El) -> El {
    let mut c = column();
    c.kids = (0..n).map(&mut f).collect();
    c
}

impl El {
    /// The fill of a box, the ink of a text: a colour (straight alpha, red low) or any binding.
    pub fn bg(mut self, v: impl Into<Val>) -> El {
        self.fill = v.into();
        self
    }
    pub fn color(self, v: impl Into<Val>) -> El {
        self.bg(v)
    }
    /// The fill while the pointer is over the element.
    pub fn hover_bg(mut self, color: u32) -> El {
        self.hover = Some(color);
        self.solid = true;
        self
    }
    /// The fill follows its binding over `ms` milliseconds instead of jumping.
    pub fn transition(mut self, ms: u32) -> El {
        self.transition_ms = ms;
        self
    }
    pub fn rounded(mut self, r_pt: f32) -> El {
        self.radius = r_pt;
        self
    }
    pub fn border(mut self, w_pt: f32, color: impl Into<Val>) -> El {
        self.border = (w_pt, color.into());
        self
    }
    /// 0 ..= 255.
    pub fn opacity(mut self, v: impl Into<Val>) -> El {
        self.opacity = v.into();
        self
    }
    /// Shadows, a gradient, a backdrop blur, a blur of the box: each field a binding.
    pub fn effect(mut self, e: Effect) -> El {
        self.effect = Some(e);
        self
    }
    pub fn text_scale(mut self, s: u32) -> El {
        self.text_scale = s.max(1);
        self
    }
    pub fn child(mut self, c: El) -> El {
        self.kids.push(c);
        self
    }
    pub fn children(mut self, c: impl IntoIterator<Item = El>) -> El {
        self.kids.extend(c);
        self
    }
    /// The children are a document of their own, moved by (`sx`, `sy`) device pixels and cut to this box.
    pub fn scroll(mut self, sx: impl Into<Val>, sy: impl Into<Val>) -> El {
        self.scroll = Some((sx.into(), sy.into()));
        self.solid = true;
        self
    }
    /// Whether the children are cut to this box: a binding, so it can change at run time.
    pub fn clip_overflow(mut self, on: impl Into<Val>) -> El {
        self.clip = Some(on.into());
        self
    }
    pub fn on_click(mut self, f: impl FnMut(&mut StateBlock) + 'static) -> El {
        self.click = Some(Box::new(f));
        self.solid = true;
        self
    }
    /// A paint offset in device pixels (layout does not move).
    pub fn translate(mut self, dx: impl Into<Val>, dy: impl Into<Val>) -> El {
        self.shift = Some((dx.into(), dy.into()));
        self
    }
    /// A horizontal paint offset of `slot * per_unit_q8 / 256` device pixels (a state word scaled per element).
    pub fn translate_x_scaled(mut self, slot: Slot<i32>, per_unit_q8: i32) -> El {
        self.shift_scaled = Some((slot, per_unit_q8));
        self
    }
}

/// A mounted description: the world, and what its clicks run.
pub struct FlatUi {
    pub world: FlatWorld,
    actions: Vec<Action>,
    scale: f32,
    /// Thread instructions of the last `mount`: building the layout tree, laying it out, describing the
    /// records, committing the world.
    pub mount_cost: [u64; 4],
}

struct Lay {
    tree: Tree,
    out: Vec<[i32; 4]>,
    at: usize,
}

impl FlatUi {
    /// A window of `w` x `h` device pixels; `scale` device pixels per point.
    pub fn new(w: i32, h: i32, scale: f32) -> FlatUi {
        FlatUi { world: FlatWorld::new(w, h), actions: Vec::new(), scale, mount_cost: [0; 4] }
    }
    fn open(&self, e: &El, t: &mut Tree) {
        let s = crate::lower::lay_spec(&e.spec, self.scale);
        if let Some((_, chars)) = &e.text {
            let (cw, ch) = cell(e.text_scale);
            t.line(s, *chars as f32 * cw as f32, ch as f32);
            return;
        }
        let i = t.open(s);
        let mut n = 0;
        if e.scroll.is_none() {
            for k in &e.kids {
                self.open(k, t);
                n += 1;
            }
        }
        t.close(i, n);
    }
    fn offset(&mut self, base: i32, by: Val) -> Val {
        match by {
            Val::Const(c) => Val::Const((base + c as i32) as u32),
            Val::Slot(s) => {
                let mut b = self.world.tape.build();
                let (a, o) = (b.konst(base), b.slot(bough_flat::SlotId(s)));
                let r = b.add(a, o);
                b.finish(r)
            }
            _ => panic!("a paint offset is a constant or a state word"),
        }
    }
    /// Emit `e` (element `lay.at` of the laid-out document) and its children into `space`, whose origin is at
    /// `base` in the document's coordinates.
    fn emit(&mut self, e: El, lay: &mut Lay, space: SpaceId, base: (i32, i32), depth: u32) {
        let r = lay.out[lay.at];
        lay.at += 1;
        let rect = Rect::new(r[0] - base.0, r[1] - base.1, r[2] - base.0, r[3] - base.1);
        let El { fill, hover, transition_ms, border, radius, opacity, effect, text, text_scale, kids, scroll, clip, click, shift, shift_scaled, solid, .. } = e;
        let w = &mut self.world;
        let mut depth = depth;
        let mut site = bough_flat::NO_SITE;
        let mut paint = fill;
        if let Some(on) = hover {
            site = w.add_site(depth.min(7));
            depth += 1;
            paint = match fill {
                Val::Const(off) => w.hover_sel(site, off, on),
                Val::Palette { slot, theme_slot } => Val::HoverPal { site, slot, theme_slot, on },
                Val::Slot(s) => {
                    let mut b = w.tape.build();
                    let (h, on, off) = (b.hovered(site), b.konst(on), b.slot(bough_flat::SlotId(s)));
                    let r = b.select(h, on, off);
                    b.finish(r)
                }
                _ => panic!("hover_bg over an expression: put the hover in the expression (`ExprBuilder::hovered`)"),
            };
        }
        if transition_ms != 0 {
            paint = w.add_anim(paint, transition_ms, true, false);
        }
        let handler = match click {
            Some(f) => {
                self.actions.push(f);
                self.actions.len() as u32
            }
            None => 0,
        };
        let painted = !matches!(paint, Val::Const(c) if c >> 24 == 0) || border.0 > 0.0 || effect.is_some();
        let needed = painted || solid || scroll.is_some() || clip.is_some();
        let mut port = None;
        if needed {
            let mut d = RecordDesc::rect(space, rect, paint);
            d.kind = if text.is_some() { Kind::Text } else if painted { Kind::Box } else { Kind::None };
            d.text = text.map(|t| t.0);
            d.text_scale = text_scale;
            d.radius = Val::Const((radius * self.scale * 256.0) as u32);
            if border.0 > 0.0 {
                d.border_w = Val::Const(((border.0 * self.scale).max(1.0) * 256.0) as u32);
                d.border = border.1;
            }
            d.opacity = opacity;
            d.effect = effect;
            d.hit = solid;
            d.site = site;
            d.handler = handler;
            if let Some((dx, dy)) = shift {
                d.x = self.offset(rect.x0, dx);
                d.y = self.offset(rect.y0, dy);
            }
            if let Some((slot, k)) = shift_scaled {
                let mut b = self.world.tape.build();
                let (base, v, k) = (b.konst(rect.x0), b.slot(slot), b.konst(k));
                let m = b.mul_q8(v, k);
                let x = b.add(base, m);
                d.x = b.finish(x);
            }
            port = Some(self.world.add_record(d));
        }
        match scroll {
            Some((sx, sy)) => {
                // the children are a document of their own, as wide as the port
                let inner = self.world.add_space(space, port, clip.unwrap_or(Val::Const(1)), sx, sy);
                let mut t = Tree::new();
                let mut root = Spec::new(true, true);
                root.w = crate::layout::Dim::Pct(1.0);
                let i = t.open(crate::lower::lay_spec(&root, self.scale));
                for k in &kids {
                    self.open(k, &mut t);
                }
                t.close(i, kids.len() as u32);
                let mut sub = Lay { out: vec![[0; 4]; t.els.len()], tree: t, at: 1 };
                sub.tree.place_row(0, self.scale, &bough_dataflow::expr::U::default(), (rect.x1 - rect.x0) as f32, &mut sub.out[..]);
                for k in kids {
                    self.emit(k, &mut sub, inner, (0, 0), depth);
                }
            }
            None => {
                let (space, base) = match clip {
                    Some(on) => (self.world.add_space(space, port, on, 0, 0), (r[0], r[1])),
                    None => (space, base),
                };
                for k in kids {
                    self.emit(k, lay, space, base, depth);
                }
            }
        }
    }
    /// Lay `root` out in the window and describe it to the world (on top of what is already there).
    pub fn mount(&mut self, root: El) {
        let now = bough_sys::perf::thread_instructions;
        let t0 = now();
        let mut t = Tree::new();
        self.open(&root, &mut t);
        let (w, h) = self.world.size();
        let mut lay = Lay { out: vec![[0; 4]; t.els.len()], tree: t, at: 0 };
        let t1 = now();
        lay.tree.place_root(0, self.scale, &bough_dataflow::expr::U::default(), [w as f32, h as f32], &mut lay.out[..]);
        let t2 = now();
        self.emit(root, &mut lay, ROOT, (0, 0), 0);
        let t3 = now();
        self.world.commit();
        self.mount_cost = [t1 - t0, t2 - t1, t3 - t2, now() - t3];
    }
    pub fn pointer_move(&mut self, x: i32, y: i32) {
        self.world.pointer_move(x, y);
    }
    pub fn pointer_leave(&mut self) {
        self.world.pointer_leave();
    }
    /// A click at (x, y): the topmost element with an action under it runs. Returns whether one did.
    pub fn click(&mut self, x: i32, y: i32) -> bool {
        match self.world.handler_at(x, y) {
            0 => false,
            h => {
                (self.actions[h as usize - 1])(&mut self.world.state);
                true
            }
        }
    }
    pub fn frame(&mut self) -> FrameInfo {
        self.world.frame()
    }
    /// A frame held to the flat core's oracles.
    pub fn check(&mut self) -> Result<FrameInfo, String> {
        self.world.check(&mut NoExt)
    }
}
