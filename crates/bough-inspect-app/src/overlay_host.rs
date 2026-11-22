//! The inspector mounted beside a live comparison app: a side panel that is a second `App` over the session's
//! own snapshot, composited onto the app's frame.
//!
//! Closed, [`WithOverlay::shot`] returns exactly the frame the plain [`Lab`] returns (no paint, no copy, no
//! snapshot work: `Session` publishes only while the host says the panel is open). Open, the panel is painted
//! with the element-layer overlay ([`bough_inspect::ui::Inspector`]), the selection's rect is outlined on the
//! app's own pixels, and the last turn's damage is flashed.
//!
//! Keys are real injected input: ` toggles, Escape closes, Up/Down move the selection by one node, Enter jumps
//! to the selected node's source (leaving a `file:line:col` in `Inspector::jump`), Left selects the parent.
//! Pointer input over the panel is translated into the panel's coordinates, so a click on a tree line is the
//! overlay's own click handler.
use crate::inspect::Fields;
use crate::lab::{Event, Lab, Scenario};
use bough_inspect::Frame;
use bough_inspect::schema::{Loc, Snapshot};
use bough_inspect::session::{FieldDef, Input, Program};
use bough_inspect::ui::Inspector;
use bough_span::Spans;
use bough_x::el::App;
use bough_x::event::Key as EKey;
use bough_x::soft::Soft;

/// Width of the side panel, in device pixels.
pub const PANEL_W: i32 = 620;

/// The panel: one overlay `App` and the selection state it reads.
pub struct Overlay {
    pub app: App<Inspector, Soft>,
    pub open: bool,
    pub width: i32,
    pub height: i32,
}

impl Overlay {
    pub fn new(snap: Snapshot, width: i32, height: i32) -> Overlay {
        let mut insp = Inspector::new(snap);
        insp.flash = true;
        Overlay {
            app: App::<Inspector, Soft>::new(insp, width, height, 1.0),
            open: true,
            width,
            height,
        }
    }
    /// A new snapshot: the selection follows its entity; damaged rows alternate their flash.
    pub fn update(&mut self, snap: Snapshot) {
        self.app.turn(move |v| {
            v.set_snapshot(snap);
            v.flash = !v.flash;
        });
    }
    pub fn snapshot(&self) -> &Snapshot {
        &self.app.v.snap
    }
    pub fn selected(&self) -> Option<usize> {
        self.app.v.selected_index
    }
    /// The last `file:line:col` a person asked to open.
    pub fn jump(&self) -> Option<Loc> {
        self.app.v.jump
    }
    pub fn select_index(&mut self, i: usize) {
        self.app.turn(move |v| v.select(i));
    }
    pub fn select_parent(&mut self) {
        self.app.turn(|v| {
            v.select_parent();
        });
    }
    /// One key. Returns whether the panel consumed it.
    pub fn key(&mut self, key: EKey, _mods: bough_x::event::Mods) -> bool {
        match key {
            EKey::Char('`') => {
                self.open = !self.open;
                true
            }
            EKey::Escape => {
                self.open = false;
                true
            }
            _ if !self.open => false,
            EKey::Down => {
                let n = self.snapshot().nodes.len();
                if n > 0 {
                    let i = self.selected().map_or(0, |i| (i + 1).min(n - 1));
                    self.select_index(i);
                }
                true
            }
            EKey::Up => {
                let n = self.snapshot().nodes.len();
                if n > 0 {
                    let i = self.selected().map_or(0, |i| i.saturating_sub(1));
                    self.select_index(i);
                }
                true
            }
            EKey::Left => {
                self.select_parent();
                true
            }
            EKey::Enter => {
                self.app.turn(|v| {
                    v.jump_to_selected();
                });
                true
            }
            _ => false,
        }
    }
    /// A pointer event in window coordinates, when it is over the panel.
    pub fn pointer(&mut self, e: &Event) -> bool {
        if !self.open {
            return false;
        }
        let x0 = self.x0();
        let e = match *e {
            Event::Pointer { x, y } if x >= x0 => Event::Pointer { x: x - x0, y },
            Event::Down { x, y, button } if x >= x0 => Event::Down {
                x: x - x0,
                y,
                button,
            },
            Event::Up { x, y, button } if x >= x0 => Event::Up {
                x: x - x0,
                y,
                button,
            },
            Event::Click { x, y } if x >= x0 => Event::Click { x: x - x0, y },
            Event::Wheel { x, y, dx, dy } if x >= x0 => Event::Wheel {
                x: x - x0,
                y,
                dx,
                dy,
            },
            _ => return false,
        };
        let a = &mut self.app;
        match e {
            Event::Pointer { x, y } => {
                a.world.pointer(&mut a.v, x, y);
            }
            Event::Down { x, y, button } => {
                a.world.pointer_down(&mut a.v, x, y, button);
            }
            Event::Up { x, y, button } => {
                a.world.pointer_up(&mut a.v, x, y, button);
            }
            Event::Click { x, y } => {
                a.world.click(&mut a.v, x, y);
            }
            _ => {}
        }
        a.settle();
        true
    }
    /// Where the panel starts in the window.
    pub fn x0(&self) -> i32 {
        (self.app.v.snap.app.w - self.width).max(0)
    }
    /// Paint the panel onto the app's frame, and the app-side overlays (selection outline, damage flash).
    pub fn compose(&mut self, f: &mut Frame) {
        if !self.open {
            return;
        }
        let x0 = (f.w - self.width).max(0);
        let w = self.width.min(f.w);
        let h = self.height.min(f.h);
        let mut soft = Soft::new(w, h);
        self.app.world.paint(&mut soft);
        for y in 0..h {
            let dst = (y * f.w + x0) as usize;
            let src = (y * w) as usize;
            f.px[dst..dst + w as usize].copy_from_slice(&soft.px[src..src + w as usize]);
        }
        // the selected record's rect, outlined on the app's own pixels (clipped to the app's side)
        if let Some(i) = self.app.v.selected_index
            && let Some(n) = self.app.v.snap.nodes.get(i)
        {
            let r = n.screen;
            let color = 0xff5a9bff;
            let (x1, y1) = (r.x1.min(x0), r.y1.min(f.h));
            let (ax, ay) = (r.x0.max(0), r.y0.max(0));
            if x1 > ax && y1 > ay {
                for t in 0..2 {
                    let y = ay + t;
                    if y < y1 {
                        for x in ax..x1 {
                            f.px[(y * f.w + x) as usize] = color;
                        }
                    }
                    let y = y1 - 1 - t;
                    if y >= ay {
                        for x in ax..x1 {
                            f.px[(y * f.w + x) as usize] = color;
                        }
                    }
                    let x = ax + t;
                    if x < x1 {
                        for y in ay..y1 {
                            f.px[(y * f.w + x) as usize] = color;
                        }
                    }
                    let x = x1 - 1 - t;
                    if x >= ax {
                        for y in ay..y1 {
                            f.px[(y * f.w + x) as usize] = color;
                        }
                    }
                }
            }
        }
        // the last turn's damage, flashed on alternate snapshots
        if self.app.v.flash {
            let color = 0xffe06020;
            let snap = &self.app.v.snap;
            for r in &snap.damage.rects {
                let (x1, y1) = (r.x1.min(x0), r.y1.min(f.h));
                let (ax, ay) = (r.x0.max(0), r.y0.max(0));
                if x1 <= ax || y1 <= ay {
                    continue;
                }
                for x in ax..x1 {
                    f.px[(ay * f.w + x) as usize] = color;
                    f.px[((y1 - 1) * f.w + x) as usize] = color;
                }
                for y in ay..y1 {
                    f.px[(y * f.w + ax) as usize] = color;
                    f.px[(y * f.w + x1 - 1) as usize] = color;
                }
            }
        }
    }
}

/// A comparison app with the inspector panel beside it, as one `Program` the session can drive.
pub struct WithOverlay<V: Scenario + 'static> {
    pub lab: Lab<V>,
    pub ov: Option<Overlay>,
    /// Open on start (the `--overlay` flag); keys toggle the same state later.
    pub start_open: bool,
}

impl<V: Scenario> WithOverlay<V> {
    pub fn new(lab: Lab<V>, open: bool) -> WithOverlay<V> {
        WithOverlay {
            lab,
            ov: None,
            start_open: open,
        }
    }
}

impl<V: Fields> Program for WithOverlay<V> {
    type V = V;
    type B = Spans<'static>;
    fn name(&self) -> &'static str {
        <V as Scenario>::NAME
    }
    fn generation(&self) -> u64 {
        self.lab.generation
    }
    fn view(&self) -> &V {
        &self.lab.scene.v
    }
    fn view_mut(&mut self) -> &mut V {
        &mut self.lab.scene.v
    }
    fn world(&self) -> &bough_x::world::World<V, Spans<'static>> {
        &self.lab.scene.world
    }
    fn world_mut(&mut self) -> &mut bough_x::world::World<V, Spans<'static>> {
        &mut self.lab.scene.world
    }
    fn settle(&mut self) {
        self.lab.settle();
    }
    fn size(&self) -> (i32, i32) {
        self.lab.size()
    }
    fn shot(&mut self) -> Frame {
        let mut f = <Lab<V> as Program>::shot(&mut self.lab);
        if let Some(ov) = &mut self.ov {
            ov.compose(&mut f);
        }
        f
    }
    fn resize(&mut self, w: i32, h: i32, scale: f32) {
        <Lab<V> as Program>::resize(&mut self.lab, w, h, scale);
        if let Some(ov) = &mut self.ov {
            ov.width = PANEL_W.min(w);
            ov.height = h;
            ov.app = App::<Inspector, Soft>::new(
                Inspector::new(ov.snapshot().clone()),
                ov.width,
                ov.height,
                1.0,
            );
        }
    }
    fn input(&mut self, e: &Input) {
        match e {
            Input::Key { key, mods } => {
                if let Some(ov) = &mut self.ov
                    && ov.key(*key, *mods)
                {
                    return;
                }
                if *key == EKey::Char('`') {
                    let open = self.ov.as_ref().is_some_and(|o| o.open);
                    self.set_open(!open);
                    return;
                }
                // Up/Down/Left/Enter with no panel: the app's own keys
                <Lab<V> as Program>::input(&mut self.lab, e);
            }
            Input::Pointer { x, y } => {
                if !self.panel_event(e) {
                    <Lab<V> as Program>::input(&mut self.lab, e);
                }
                let _ = (x, y);
            }
            Input::Down { .. } | Input::Up { .. } | Input::Click { .. } => {
                if !self.panel_event(e) {
                    <Lab<V> as Program>::input(&mut self.lab, e);
                }
            }
            Input::Wheel { .. } => {
                if !self.panel_event(e) {
                    <Lab<V> as Program>::input(&mut self.lab, e);
                }
            }
            _ => <Lab<V> as Program>::input(&mut self.lab, e),
        }
    }
    fn fields(&self) -> &'static [FieldDef<Self::V>] {
        <V as Fields>::fields_def()
    }
    fn reload(&mut self) -> bool {
        <Lab<V> as Program>::reload(&mut self.lab)
    }
    fn fresh(&self) -> Option<Self> {
        <Lab<V> as Program>::fresh(&self.lab).map(|lab| WithOverlay::new(lab, false))
    }
    /// The overlay presents the same live animations as the app underneath it.
    fn animations(&self, snap: &bough_inspect::Snapshot) -> Vec<bough_inspect::protocol::Animation> {
        <Lab<V> as Program>::animations(&self.lab, snap)
    }
    fn overlay(&self) -> bool {
        #[cfg(feature = "mutant_overlay_paint")]
        {
            return true;
        }
        self.ov.as_ref().map_or(self.start_open, |o| o.open)
    }
    fn publish(&mut self, snap: &Snapshot) {
        if self.ov.as_ref().is_some_and(|o| !o.open) {
            return;
        }
        let snap = snap.clone();
        match &mut self.ov {
            Some(ov) => ov.update(snap),
            None => {
                let (w, h) = self.lab.size();
                self.ov = Some(Overlay::new(snap, PANEL_W.min(w), h));
            }
        }
    }
}

impl<V: Fields> WithOverlay<V> {
    fn set_open(&mut self, open: bool) {
        match &mut self.ov {
            Some(ov) => ov.open = open,
            None if open => self.start_open = true,
            None => {}
        }
    }
    fn panel_event(&mut self, e: &Input) -> bool {
        let ev = match *e {
            Input::Pointer { x, y } => Event::Pointer { x, y },
            Input::Down { x, y, button } => Event::Down { x, y, button },
            Input::Up { x, y, button } => Event::Up { x, y, button },
            Input::Click { x, y } => Event::Click { x, y },
            Input::Wheel { x, y, dx, dy, .. } => Event::Wheel {
                x,
                y,
                dx,
                dy: dy as f32,
            },
            _ => return false,
        };
        self.ov.as_mut().is_some_and(|ov| ov.pointer(&ev))
    }
}
