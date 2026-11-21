//! The real comparison apps on a headless host: no inspector dependency, so a build with the `inspect` feature
//! off is the zero-cost control.
use bough_guibench_x::scene::{Model, Scene};
use bough_guibench_x::{rows, search, spinner, table};
use bough_host::App;
use bough_x::event::{Button, Key, Mods};

/// A painted frame, in the host's own terms (the inspector wraps it as its `Frame`).
#[derive(Clone, Debug, PartialEq)]
pub struct Shot {
    pub w: i32,
    pub h: i32,
    pub px: Vec<u32>,
}

/// An input event the host injects (the same set `bough-inspect` sends, in this crate's terms).
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Pointer { x: i32, y: i32 },
    Down { x: i32, y: i32, button: Button },
    Up { x: i32, y: i32, button: Button },
    Click { x: i32, y: i32 },
    Key { key: Key, mods: Mods },
    Text { text: String },
    Wheel { x: i32, y: i32, dx: f32, dy: f32 },
}

/// A scenario app with a way to build a fresh one (record/replay).
pub trait Scenario: Model + 'static {
    const NAME: &'static str;
    fn rebuild(&self) -> Self;
}

/// One scenario, mounted and painted by the benchmark app's own `Scene`.
pub struct Lab<V: Scenario> {
    pub scene: Scene<V>,
}
impl<V: Scenario> Lab<V> {
    pub fn new(v: V, w: i32, h: i32, scale: i32) -> Lab<V> {
        let mut scene = Scene::new(v);
        // the host's first resize mounts the world
        App::resize(&mut scene, w, h, scale.max(1));
        Lab { scene }
    }
    pub fn size(&self) -> (i32, i32) {
        (self.scene.geo.w, self.scene.geo.h)
    }
    pub fn scale(&self) -> i32 {
        self.scene.geo.k
    }
    pub fn settle(&mut self) {
        App::settle(&mut self.scene);
    }
    pub fn advance(&mut self, ms: u32) {
        App::advance(&mut self.scene, ms);
    }
    pub fn resize(&mut self, w: i32, h: i32, scale: i32) {
        App::resize(&mut self.scene, w, h, scale.max(1));
    }
    pub fn shot(&mut self) -> Shot {
        Shot {
            w: self.scene.geo.w,
            h: self.scene.geo.h,
            px: self.scene.full(),
        }
    }
    pub fn input(&mut self, e: &Event) {
        let v = &mut self.scene.v;
        let w = &mut self.scene.world;
        match e {
            Event::Pointer { x, y } => {
                w.pointer(v, *x, *y);
            }
            Event::Down { x, y, button } => {
                w.pointer_down(v, *x, *y, *button);
            }
            Event::Up { x, y, button } => {
                w.pointer_up(v, *x, *y, *button);
            }
            Event::Click { x, y } => {
                w.click(v, *x, *y);
            }
            Event::Key { key, mods } => {
                // the host's rule: a printable character with no command modifier is text if a handler takes
                // it, and the key otherwise
                if let Key::Char(c) = *key
                    && !mods.ctrl
                    && !mods.alt
                    && !mods.cmd
                    && !c.is_control()
                {
                    let mut b = [0u8; 4];
                    if w.text(v, c.encode_utf8(&mut b)).consumed {
                        return;
                    }
                }
                w.key_down(v, *key, *mods);
            }
            Event::Text { text } => {
                w.text(v, text);
            }
            Event::Wheel { x, y, dy, .. } => {
                v.wheel(*x, *y, *dy as i32);
            }
        }
    }
    pub fn rebuild_with(&self, v: V) -> Lab<V> {
        Lab::new(v, self.scene.geo.w, self.scene.geo.h, self.scene.geo.k)
    }
}

// ---- the four scenarios' rebuilds ----------------------------------------------------------------------------

impl Scenario for rows::Rows {
    const NAME: &'static str = "rows";
    fn rebuild(&self) -> Self {
        rows::Rows::new(self.count)
    }
}
impl Scenario for table::Table {
    const NAME: &'static str = "table";
    fn rebuild(&self) -> Self {
        table::Table::new(self.count, self.ticker)
    }
}
impl Scenario for search::Search {
    const NAME: &'static str = "search";
    fn rebuild(&self) -> Self {
        search::Search::new(self.rows.len(), self.filter)
    }
}
impl Scenario for spinner::Spinner {
    const NAME: &'static str = "spinner";
    fn rebuild(&self) -> Self {
        spinner::Spinner::new(self.rows.len())
    }
}
