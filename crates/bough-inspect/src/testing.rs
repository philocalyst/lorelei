//! Typed selectors and a session wrapper, so an application's own tests are three lines:
//!
//! ```ignore
//! let mut app = bough_inspect::testing::TestApp::new(rows::Rows::new(1_000));
//! let cell = app.find().kind("text").text("Row 00042").one();
//! app.click(cell);
//! assert_eq!(app.node(cell).kind, "text");
//! ```
//!
//! A selector walks the snapshot the inspector publishes, not a second description of the view: what the test
//! names is what the inspector and an agent see.
use crate::frame::Frame;
use crate::record::Command;
use crate::schema::{Key, Node, Snapshot};
use crate::session::{Program, Session};
use bough_dataflow::event::{Key as K, Mods};

pub struct Find<'a> {
    snap: &'a Snapshot,
    kind: Option<&'a str>,
    ty: Option<&'a str>,
    text: Option<&'a str>,
    within: Option<usize>,
    visible: Option<bool>,
    limit: usize,
}
impl<'a> Find<'a> {
    pub fn kind(mut self, k: &'a str) -> Self {
        self.kind = Some(k);
        self
    }
    pub fn ty(mut self, t: &'a str) -> Self {
        self.ty = Some(t);
        self
    }
    pub fn text(mut self, t: &'a str) -> Self {
        self.text = Some(t);
        self
    }
    pub fn within(mut self, store: usize) -> Self {
        self.within = Some(store);
        self
    }
    pub fn visible(mut self, v: bool) -> Self {
        self.visible = Some(v);
        self
    }
    pub fn limit(mut self, n: usize) -> Self {
        self.limit = n;
        self
    }
    fn matches(&self, n: &Node) -> bool {
        self.kind.is_none_or(|k| n.kind == k)
            && self.ty.is_none_or(|t| n.ty.contains(t))
            && self
                .text
                .is_none_or(|t| n.text.as_deref().is_some_and(|x| x.contains(t)))
            && self.within.is_none_or(|s| n.store == s)
            && self.visible.is_none_or(|v| n.visible == v)
    }
    /// The matching node indices, in paint order.
    pub fn all(&self) -> Vec<usize> {
        self.snap
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| self.matches(n))
            .map(|(i, _)| i)
            .take(self.limit)
            .collect()
    }
    pub fn count(&self) -> usize {
        self.snap.nodes.iter().filter(|n| self.matches(n)).count()
    }
    pub fn first(&self) -> Option<usize> {
        self.snap.nodes.iter().position(|n| self.matches(n))
    }
    /// The one match; panics (a test's assertion) when there is none or more than one.
    #[track_caller]
    pub fn one(&self) -> usize {
        let all = self.all();
        assert_eq!(
            all.len(),
            1,
            "{} nodes match the selector, not one",
            all.len()
        );
        all[0]
    }
    pub fn none(&self) {
        assert_eq!(self.count(), 0, "a node matches the selector");
    }
}

pub struct TestApp<P: Program> {
    pub session: Session<P>,
}
impl<P: Program> TestApp<P> {
    pub fn new(app: P) -> TestApp<P> {
        TestApp {
            session: Session::new(app),
        }
    }
    pub fn snapshot(&mut self) -> &Snapshot {
        self.session.snapshot()
    }
    pub fn find(&mut self) -> Find<'_> {
        Find {
            snap: self.session.snapshot(),
            kind: None,
            ty: None,
            text: None,
            within: None,
            visible: None,
            limit: 256,
        }
    }
    #[track_caller]
    pub fn node(&mut self, i: usize) -> &Node {
        &self.session.snapshot().nodes[i]
    }
    pub fn key(&mut self, i: usize) -> Key {
        self.session.snapshot().nodes[i].key
    }
    /// The centre of a node on screen, in the store space the world's input uses.
    pub fn centre(&mut self, i: usize) -> (i32, i32) {
        let r = self.session.snapshot().nodes[i].screen;
        ((r.x0 + r.x1) / 2, (r.y0 + r.y1) / 2)
    }
    pub fn click(&mut self, i: usize) {
        let (x, y) = self.centre(i);
        self.session.run(&Command::Click { x, y });
    }
    pub fn pointer(&mut self, i: usize) {
        let (x, y) = self.centre(i);
        self.session.run(&Command::Pointer { x, y });
    }
    pub fn key_press(&mut self, k: K) {
        self.session.run(&Command::Key {
            key: k,
            mods: Mods::default(),
        });
    }
    pub fn text(&mut self, s: &str) {
        self.session.run(&Command::Text { text: s.to_owned() });
    }
    pub fn wheel(&mut self, x: i32, y: i32, dy: f32) {
        self.session.run(&Command::Wheel { x, y, dx: 0.0, dy });
    }
    pub fn tick(&mut self, ms: u32) {
        self.session.run(&Command::Tick { ms });
    }
    pub fn shot(&mut self) -> Frame {
        self.session.shot()
    }
    /// The text of the one node a selector names, as a test reads it.
    #[track_caller]
    pub fn text_of(&mut self, i: usize) -> String {
        self.session.snapshot().nodes[i]
            .text
            .clone()
            .unwrap_or_default()
    }
    /// Whether some node's text contains `s`.
    pub fn has_text(&mut self, s: &str) -> bool {
        self.session
            .snapshot()
            .nodes
            .iter()
            .any(|n| n.text.as_deref().is_some_and(|t| t.contains(s)))
    }
}
