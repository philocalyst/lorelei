//! The in-application overlay: the tree, the selection, source locations, properties, dependencies, damage
//! flashes and timings, written with the typed core's element layer (`bough_x::el`) — the façade's own idiom.
//!
//! The overlay is a plain view over one [`Snapshot`] (the inspector's, not a second description): a host that has a
//! snapshot calls [`Inspector::set_snapshot`], then describes it. It never writes the inspected world; a source
//! location a person clicks is left in [`Inspector::jump`] for the host to open in an editor.
//!
//! What an owner decided is not here: no style editors, no property contribution model (a record's operand is a
//! staged expression of state; the overlay shows it and does not pretend to write it).
use crate::schema::{Key, Loc, Node, PropValue, Snapshot};
use bough_x::el::prelude::*;
use bough_x::el::{Signal, View as ElView};

const ROW: f32 = 16.0;
const TREE_W: f32 = 720.0;
const MAX_TREE: usize = 400;
const MAX_DETAIL: usize = 24;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Select(usize),
    Parent,
    Jump(Loc),
    None,
}

#[derive(Clone, Debug)]
pub struct Line {
    pub action: Action,
    pub label: String,
    pub selected: bool,
    pub damaged: bool,
}

/// The overlay's state: one snapshot, one selection that survives scrolling and re-description.
pub struct Inspector {
    pub snap: Snapshot,
    /// The selected entity (its identity, not its index: indices move with the window).
    pub selected: Option<Key>,
    pub selected_index: Option<usize>,
    /// Set when the selected entity is no longer part of the view's shape (the subtree was removed).
    pub stale: bool,
    /// The last source location a person asked to open (`file:line:col`).
    pub jump: Option<Loc>,
    /// Alternate the damage marker, so a repaint is visible as a flash rather than a constant colour.
    pub flash: bool,
    pub tree_y: Signal<f32>,
    pub detail_y: Signal<f32>,
    lines: Vec<Line>,
    details: Vec<Line>,
    status: String,
}
impl Inspector {
    pub fn new(snap: Snapshot) -> Inspector {
        let mut i = Inspector {
            snap,
            selected: None,
            selected_index: None,
            stale: false,
            jump: None,
            flash: false,
            tree_y: Signal::new(0.0),
            detail_y: Signal::new(0.0),
            lines: Vec::new(),
            details: Vec::new(),
            status: String::new(),
        };
        i.rebuild();
        i
    }
    /// A new snapshot. The selection follows its entity; an entity the view's shape no longer has is dropped
    /// (a row that merely left the window is kept: it is still the same row).
    pub fn set_snapshot(&mut self, snap: Snapshot) {
        let keep = self.selected.filter(|k| exists(&snap, k));
        self.stale = self.selected.is_some() && keep.is_none();
        self.selected = keep;
        self.snap = snap;
        self.selected_index = self
            .selected
            .and_then(|k| self.snap.find_key(&k).map(|n| index_of(&self.snap, n)));
        self.rebuild();
    }
    /// Select a node index of the current snapshot.
    pub fn select(&mut self, index: usize) {
        if let Some(n) = self.snap.nodes.get(index) {
            self.selected = Some(n.key);
            self.selected_index = Some(index);
            self.stale = false;
            self.rebuild();
        }
    }
    /// Select by entity (what a test or a host with an older snapshot holds).
    pub fn select_key(&mut self, key: Key) -> bool {
        match self.snap.find_key(&key).map(|n| index_of(&self.snap, n)) {
            Some(i) => {
                self.select(i);
                true
            }
            None => {
                self.selected = Some(key);
                self.selected_index = None;
                self.stale = !exists(&self.snap, &key);
                self.rebuild();
                false
            }
        }
    }
    /// Select the nearest enclosing scope that has a record of its own; `false` at the application root
    /// (nothing above it). The core's lexical parent is the store tree (a record's own parent chain is the
    /// parent column the roadmap still owes), so a walk that reaches the root store stops.
    pub fn select_parent(&mut self) -> bool {
        let Some(i) = self.selected_index else { return false };
        let Some(n) = self.snap.nodes.get(i) else { return false };
        if self.snap.stores[n.store].depth == 0 {
            return false;
        }
        let chain = self.snap.store_chain(n.store);
        // nearest first, skipping the selected record's own store
        for &s in chain.iter().rev().skip(1) {
            if let Some(j) = self.snap.nodes.iter().position(|x| x.store == s) {
                self.select(j);
                return true;
            }
        }
        false
    }
    /// The stores from the root to the selected record's.
    pub fn ancestors(&self) -> Vec<usize> {
        match self.selected_index.and_then(|i| self.snap.nodes.get(i)) {
            Some(n) => self.snap.store_chain(n.store),
            None => Vec::new(),
        }
    }
    pub fn lines(&self) -> &[Line] {
        &self.lines
    }
    pub fn details(&self) -> &[Line] {
        &self.details
    }
    pub fn status(&self) -> &str {
        &self.status
    }
    /// Jump to the selected record's source location (what the host's editor integration opens).
    pub fn jump_to_selected(&mut self) -> Option<Loc> {
        let l = self
            .selected_index
            .and_then(|i| self.snap.nodes.get(i))
            .map(|n| n.loc)?;
        self.jump = Some(l);
        Some(l)
    }

    fn is_damaged(&self, n: &Node) -> bool {
        if self.snap.damage.tiles == 0 {
            return false;
        }
        let screen = n.screen;
        self.snap.damage.rects.iter().any(|r| {
            screen.x0 < r.x1 && r.x0 < screen.x1 && screen.y0 < r.y1 && r.y0 < screen.y1
        })
    }
    fn rebuild(&mut self) {
        let mut lines = Vec::new();
        let mut i = 0;
        for n in &self.snap.nodes {
            if i >= MAX_TREE {
                break;
            }
            i += 1;
            let indent = " ".repeat(n.key.depth as usize * 2);
            let text = n.text.as_deref().unwrap_or("");
            let mut label = format!(
                "{indent}{:<7} {}{}",
                n.kind,
                if text.is_empty() { String::new() } else { format!("{text:?} ") },
                location(&n.loc)
            );
            if !n.handlers.is_empty() {
                label.push_str(&format!(" [{}]", n.handlers.join(",")));
            }
            if !n.visible {
                label.push_str(" (offscreen)");
            }
            lines.push(Line {
                action: Action::Select(index_of(&self.snap, n)),
                selected: self.selected == Some(n.key),
                damaged: self.is_damaged(n),
                label,
            });
        }
        self.lines = lines;

        let mut details = Vec::new();
        match self.selected_index.and_then(|i| self.snap.nodes.get(i)) {
            Some(n) => {
                details.push(detail(
                    format!("{}  {}  {}", n.kind, n.ty, location(&n.loc)),
                    Action::Jump(n.loc),
                ));
                details.push(detail(
                    format!("[parent]  store {}", n.store),
                    Action::Parent,
                ));
                details.push(detail(
                    format!(
                        "rect {} {} {}x{}   screen {} {} {}x{}",
                        n.rect.x0, n.rect.y0, n.rect.w(), n.rect.h(),
                        n.screen.x0, n.screen.y0, n.screen.w(), n.screen.h()
                    ),
                    Action::None,
                ));
                if n.moving {
                    details.push(detail(
                        format!("paint offset {} {}", n.offset.0, n.offset.1),
                        Action::None,
                    ));
                }
                if let Some(t) = &n.text {
                    details.push(detail(format!("text {t:?}"), Action::None));
                }
                let words = |w: &[u32]| -> String {
                    let mut s = Vec::new();
                    for &x in w {
                        let name = self.snap.words.iter().find(|v| v.word == x).and_then(|v| v.name);
                        s.push(match name {
                            Some(n) => format!("{x}:{n}"),
                            None => x.to_string(),
                        });
                    }
                    s.join(",")
                };
                if !n.reads.is_empty() {
                    details.push(detail(format!("reads {}", words(&n.reads)), Action::None));
                }
                if !n.handlers.is_empty() {
                    details.push(detail(
                        format!(
                            "handlers {}{}",
                            n.handlers.join(","),
                            n.handler_ty.map_or_else(String::new, |t| format!(" — {t}"))
                        ),
                        Action::None,
                    ));
                }
                for p in &n.ops {
                    let expr = if p.expr.len() > 48 {
                        format!("{}…", &p.expr[..p.expr.len().min(48)])
                    } else {
                        p.expr.to_owned()
                    };
                    details.push(detail(
                        format!(
                            "{} = {}  [{}]  reads {}{}",
                            p.name,
                            p.value.text(),
                            p.stage,
                            words(&p.reads),
                            if p.reads.is_empty() { String::new() } else { String::new() }
                        )
                        .to_owned()
                        + &format!("  {expr}"),
                        Action::None,
                    ));
                }
                let chain: Vec<String> = self
                    .ancestors()
                    .iter()
                    .map(|&s| format!("{}({})", s, self.snap.stores[s].kind))
                    .collect();
                details.push(detail(format!("stores {}", chain.join(" > ")), Action::None));
            }
            None => {
                let why = if self.stale {
                    "the selected element is no longer part of the view"
                } else if self.selected.is_some() {
                    "the selected element is not in the exposed window (scroll to it)"
                } else {
                    "nothing selected: click a line"
                };
                details.push(detail(why.to_owned(), Action::None));
            }
        }
        details.push(detail(
            format!(
                "damage {} tiles   last {} (settle {} µs, paint {} µs)",
                self.snap.damage.tiles,
                self.snap.counters.last_command,
                self.snap.counters.last_settle_us,
                self.snap.counters.last_paint_us
            ),
            Action::None,
        ));
        details.push(detail(
            format!(
                "records {}   pages {}   columns {}   revision {}",
                self.snap.counters.records,
                self.snap.counters.pages,
                self.snap.stores.iter().map(|s| s.cols.len()).sum::<usize>(),
                self.snap.app.revision
            ),
            Action::None,
        ));
        details.truncate(MAX_DETAIL);
        self.details = details;
        self.status = format!(
            "{} · {}x{} @{}x · {} records · {} nodes · rev {} · {} tiles{}",
            self.snap.app.name,
            self.snap.app.w,
            self.snap.app.h,
            self.snap.app.scale,
            self.snap.counters.records,
            self.snap.nodes.len(),
            self.snap.app.revision,
            self.snap.damage.tiles,
            if self.snap.ax.is_some() { format!(" · ax {}", self.snap.ax.as_ref().unwrap().nodes.len()) } else { String::new() }
        );
    }
}

fn detail(label: String, action: Action) -> Line {
    Line {
        action,
        label,
        selected: false,
        damaged: false,
    }
}
fn location(l: &Loc) -> String {
    // the file's last two components are enough in a panel
    let mut parts: Vec<&str> = l.file.rsplit('/').take(2).collect();
    parts.reverse();
    format!("{}:{}:{}", parts.join("/"), l.line, l.col)
}
fn index_of(snap: &Snapshot, n: &Node) -> usize {
    snap.nodes.iter().position(|x| x.key == n.key).unwrap_or(0)
}
/// Whether the view's shape still has this entity. A list's row that is outside its count is gone; a row merely
/// outside the exposed window is not.
fn exists(snap: &Snapshot, k: &Key) -> bool {
    let Some(st) = snap.stores.iter().find(|s| s.path[..s.depth as usize] == k.path[..k.depth as usize]) else {
        return false;
    };
    if st.kind == "list" && k.row >= st.n {
        return false;
    }
    if snap.find_place(k.depth, k.path, k.row, k.pos).is_some() {
        return true;
    }
    // not exposed: still part of the shape when the store is
    true
}

fn colour(l: &Line, flash: bool) -> u32 {
    if l.selected {
        0xff2f3f6b
    } else if l.damaged && flash {
        0xff5a2020
    } else if l.damaged {
        0xff2a1818
    } else if l.action != Action::None {
        0xff181820
    } else {
        0xff141418
    }
}

impl ElView for Inspector {
    fn view(&self) -> impl Element<Self> + '_ {
        let flash = self.flash;
        let tree = (0..self.lines.len()).map(move |i| {
            let l = &self.lines[i];
            let action = l.action;
            let fg = if l.selected {
                0xffffffff
            } else if l.damaged {
                0xffffd0d0
            } else {
                0xffc8c8d0
            };
            row((
                div().w(3.0).h(ROW).bg(rgb(if l.damaged { 0xffff4444 } else { 0xff303040 })),
                text(l.label.clone()).font(12.0, ROW).color(rgb(fg)),
            ))
            .w_pct(1.0)
            .h(ROW)
            .bg(rgb(colour(l, flash)))
            .on_click(move |v: &mut Inspector| match action {
                Action::Select(i) => {
                    v.select(i);
                    v.flash = !v.flash;
                }
                Action::Parent => {
                    v.select_parent();
                }
                Action::Jump(l) => v.jump = Some(l),
                Action::None => {}
            })
        });
        let details = (0..self.details.len()).map(move |i| {
            let l = &self.details[i];
            let action = l.action;
            let fg = match action {
                Action::Jump(_) => 0xff8fd0ff,
                Action::Parent => 0xffffe08f,
                _ => 0xffb0b0c0,
            };
            row((text(l.label.clone()).font(12.0, ROW).color(rgb(fg)),))
                .w_pct(1.0)
                .h(ROW)
                .bg(rgb(0xff141418))
                .on_click(move |v: &mut Inspector| match action {
                    Action::Parent => {
                        v.select_parent();
                    }
                    Action::Jump(l) => v.jump = Some(l),
                    _ => {}
                })
        });
        column((
            row((
                text("lorelei inspector").font(14.0, 24.0).color(rgb(0xffe8e8f0)),
                text("  "),
                text(self.status.clone()).font(12.0, 24.0).color(rgb(0xff9aa0b8)),
            ))
            .w_pct(1.0)
            .h(24.0)
            .p(4.0)
            .bg(rgb(0xff1c1c24)),
            row((
                div()
                    .w(TREE_W)
                    .grow(1.0)
                    .h_pct(1.0)
                    .bg(rgb(0xff101014))
                    .child(column(each(tree)).grow(1.0).scroll(sig!(self.tree_y))),
                div()
                    .grow(1.0)
                    .h_pct(1.0)
                    .p(6.0)
                    .bg(rgb(0xff16161c))
                    .child(column(each(details)).grow(1.0).scroll(sig!(self.detail_y))),
            ))
            .w_pct(1.0)
            .grow(1.0),
        ))
        .w_pct(1.0)
        .h_pct(1.0)
        .bg(rgb(0xff0c0c10))
    }
}

/// A short text form of a value, for a terminal client.
pub fn value_text(v: &PropValue) -> String {
    v.text()
}
