//! The schema: one owned, versioned description of what an inspected world holds right now.
//!
//! Version 1. Every member is additive: a reader checks [`SCHEMA_VERSION`] in `hello` and may ignore members
//! it does not know. [`Snapshot::to_json`] is canonical (insertion order, no whitespace) so the golden test can
//! compare bytes.
use crate::json::{self, Value as J};
use bough_expr::{Rect as ERect, StageId};
use bough_dataflow::reflect::Val;

/// The schema this build writes. Bump on any change that is not purely additive.
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, PartialOrd, Ord, Hash)]
pub struct R {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}
impl R {
    pub const EMPTY: R = R {
        x0: 0,
        y0: 0,
        x1: 0,
        y1: 0,
    };
    pub fn of(r: ERect) -> R {
        R {
            x0: r.x0,
            y0: r.y0,
            x1: r.x1,
            y1: r.y1,
        }
    }
    pub fn w(&self) -> i32 {
        self.x1 - self.x0
    }
    pub fn h(&self) -> i32 {
        self.y1 - self.y0
    }
    pub fn empty(&self) -> bool {
        self.x0 >= self.x1 || self.y0 >= self.y1
    }
    /// The intersection: what a screenshot by element needs.
    pub fn and(self, o: R) -> R {
        R {
            x0: self.x0.max(o.x0),
            y0: self.y0.max(o.y0),
            x1: self.x1.min(o.x1),
            y1: self.y1.min(o.y1),
        }
    }
    pub(crate) fn json(&self) -> J {
        J::obj()
            .set("x", self.x0)
            .set("y", self.y0)
            .set("w", self.w())
            .set("h", self.h())
    }
}

/// A source position, as `#[track_caller]` recorded it at the view's call site.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Loc {
    pub file: &'static str,
    pub line: u32,
    pub col: u32,
}
impl Loc {
    pub fn of(l: &'static core::panic::Location<'static>) -> Loc {
        Loc {
            file: l.file(),
            line: l.line(),
            col: l.column(),
        }
    }
    /// `file:line:col`, the form an editor's jump command takes.
    pub fn short(&self) -> String {
        format!("{}:{}:{}", self.file, self.line, self.col)
    }
    pub(crate) fn json(&self) -> J {
        J::obj()
            .set("file", self.file)
            .set("line", self.line)
            .set("col", self.col)
    }
}

/// A stable identity derived from what does not move: the source site a leaf was written at, the store
/// path and row/pos inside it (logical slots, not paint order), and the host's world generation. Two
/// descriptions of the same view agree; a scroll that recycles materialisation slots does not change it; a
/// replacement world (a remount) does.
pub fn stable_id(loc: Loc, depth: u16, path: [u16; 6], row: u32, pos: u32, generation: u64) -> u64 {
    // FNV-1a over a fixed little-endian encoding; no allocator, no hasher state to leak between builds.
    const P: u64 = 0x100_0000_01b3;
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    let mut eat = |bytes: &[u8]| {
        for &b in bytes {
            h = (h ^ b as u64).wrapping_mul(P);
        }
    };
    eat(loc.file.as_bytes());
    eat(&[0xff]);
    eat(&loc.line.to_le_bytes());
    eat(&loc.col.to_le_bytes());
    eat(&[depth as u8, (depth >> 8) as u8]);
    for q in path.iter().take(depth as usize) {
        eat(&q.to_le_bytes());
    }
    eat(&row.to_le_bytes());
    eat(&pos.to_le_bytes());
    eat(&generation.to_le_bytes());
    h
}

/// A stable id for a record's kind: the call site and the paint kind. (`Node::ty` is display-only.)
pub fn kind_id(loc: Loc, kind: &str) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    let mut eat = |bytes: &[u8]| {
        for &b in bytes {
            h = (h ^ b as u64).wrapping_mul(0x100_0000_01b3);
        }
    };
    eat(loc.file.as_bytes());
    eat(&loc.line.to_le_bytes());
    eat(&loc.col.to_le_bytes());
    eat(kind.as_bytes());
    h
}

/// A stable id for one operand of one record: the record's stable id and the operand name.
pub fn operand_id(node: u64, name: &str) -> u64 {
    let mut h = node ^ 0xcbf2_9ce4_8422_2325;
    for &b in name.as_bytes() {
        h = (h ^ b as u64).wrapping_mul(0x100_0000_01b3);
    }
    h
}

/// A record's identity in the stores, stable across scrolling and re-description ([`bough_dataflow::reflect::Ent`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Key {
    pub path: [u16; 6],
    pub depth: u16,
    pub row: u32,
    pub pos: u32,
    /// The materialisation it names, when it is exposed: the column and record.
    pub col: u32,
    pub rec: u32,
}
impl Key {
    pub fn of(e: bough_dataflow::reflect::Ent, col: u32, rec: u32) -> Key {
        Key {
            path: e.path.at,
            depth: e.path.n,
            row: e.row,
            pos: e.pos,
            col,
            rec,
        }
    }
    /// The identity without its current materialisation (what survives a scroll).
    pub fn place(&self) -> (u16, [u16; 6], u32, u32) {
        (self.depth, self.path, self.row, self.pos)
    }
    pub(crate) fn json(&self) -> J {
        let p: Vec<J> = self.path[..self.depth as usize]
            .iter()
            .map(|&q| J::Int(q as i64))
            .collect();
        J::obj()
            .set("path", p)
            .set("row", self.row)
            .set("pos", self.pos)
            .set("col", self.col)
            .set("rec", self.rec)
    }
}

/// An operand's value now.
#[derive(Clone, Debug, PartialEq)]
pub enum PropValue {
    None,
    Color(u32),
    Num {
        int: i32,
        decimals: u8,
        text: String,
    },
    Text(String),
    Pair(i32, i32),
    Bool(bool),
    Float(f32),
    Int(i32),
}
impl PropValue {
    pub fn of(v: Val) -> PropValue {
        match v {
            Val::None => PropValue::None,
            Val::Color(c) => PropValue::Color(c),
            Val::Num(int, decimals) => PropValue::Num {
                int,
                decimals,
                text: v.text(),
            },
            Val::Text(_) => PropValue::Text(v.text()),
            Val::Pair(x, y) => PropValue::Pair(x, y),
        }
    }
    /// A short form for a tree line.
    pub fn text(&self) -> String {
        match self {
            PropValue::None => String::new(),
            PropValue::Color(c) => format!("#{:06x}", c & 0xff_ffff),
            PropValue::Num { text, .. } => text.clone(),
            PropValue::Text(t) => t.clone(),
            PropValue::Pair(x, y) => format!("({x}, {y})"),
            PropValue::Bool(b) => b.to_string(),
            PropValue::Float(f) => {
                if f.fract() == 0.0 {
                    format!("{}", *f as i64)
                } else {
                    format!("{f}")
                }
            }
            PropValue::Int(i) => i.to_string(),
        }
    }
    /// The value's type, as JSON says it.
    pub fn kind(&self) -> &'static str {
        match self {
            PropValue::None => "none",
            PropValue::Color(_) => "color",
            PropValue::Num { .. } => "num",
            PropValue::Text(_) => "text",
            PropValue::Pair(..) => "pair",
            PropValue::Bool(_) => "bool",
            PropValue::Float(_) => "float",
            PropValue::Int(_) => "int",
        }
    }
    pub(crate) fn json(&self) -> J {
        match self {
            PropValue::None => J::obj().set("kind", "none"),
            PropValue::Color(c) => J::obj()
                .set("kind", "color")
                .set("rgba", *c)
                .set("text", format!("#{:08x}", c)),
            PropValue::Num {
                int,
                decimals,
                text,
            } => J::obj()
                .set("kind", "num")
                .set("int", *int)
                .set("decimals", *decimals as i64)
                .set("text", text.as_str()),
            PropValue::Text(t) => J::obj().set("kind", "text").set("text", t.as_str()),
            PropValue::Pair(x, y) => J::obj().set("kind", "pair").set("x", *x).set("y", *y),
            PropValue::Bool(b) => J::obj().set("kind", "bool").set("value", *b),
            PropValue::Float(f) => J::obj().set("kind", "float").set("value", *f),
            PropValue::Int(i) => J::obj().set("kind", "int").set("value", *i),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Prop {
    /// A stable operand id within its record (derived from the node id and the operand's name): the `expr`
    /// beside it is a compiler-written type and display-only.
    pub id: u64,
    pub name: &'static str,
    /// The bytes of the record that hold it.
    pub word: u32,
    pub words: u32,
    pub value: PropValue,
    /// `const`, `mount`, `turn`, `frame`.
    pub stage: &'static str,
    /// The expression's type, written out by the compiler: the lenses and functions it was built from.
    pub expr: &'static str,
    /// The block words it reads.
    pub reads: Vec<u32>,
}
impl Prop {
    pub(crate) fn json(&self) -> J {
        J::obj()
            .set("id", format!("{:016x}", self.id))
            .set("name", self.name)
            .set("value", self.value.json())
            .set("stage", self.stage)
            .set("expr", self.expr)
            .set("word", self.word)
            .set("words", self.words)
            .set(
                "reads",
                self.reads
                    .iter()
                    .map(|&w| J::Int(w as i64))
                    .collect::<Vec<_>>(),
            )
    }
}

pub fn stage_name(s: StageId) -> &'static str {
    match s {
        StageId::Const => "const",
        StageId::Mount => "mount",
        StageId::Turn => "turn",
        StageId::Frame => "frame",
    }
}

/// What a column's archetype is (only its type knows), and what of it this snapshot used.
#[derive(Clone, Debug, PartialEq)]
pub struct Col {
    pub index: u32,
    pub kind: &'static str,
    pub ty: &'static str,
    pub stage: &'static str,
    pub reads: Vec<u32>,
    pub moves: bool,
    pub timed: bool,
    pub npages: u32,
    pub nrec: u32,
    /// Positions of the store's shape that share this column.
    pub sites: Vec<u32>,
    pub records: usize,
    pub bytes: usize,
}
impl Col {
    pub(crate) fn json(&self) -> J {
        J::obj()
            .set("index", self.index)
            .set("kind", self.kind)
            .set("ty", self.ty)
            .set("stage", self.stage)
            .set(
                "reads",
                self.reads
                    .iter()
                    .map(|&w| J::Int(w as i64))
                    .collect::<Vec<_>>(),
            )
            .set("moves", self.moves)
            .set("timed", self.timed)
            .set("pages", self.npages)
            .set("nrec", self.nrec)
            .set(
                "sites",
                self.sites
                    .iter()
                    .map(|&s| J::Int(s as i64))
                    .collect::<Vec<_>>(),
            )
            .set("records", self.records)
            .set("bytes", self.bytes)
    }
}

/// A store of the tree: the root shape, a scope, or a list.
#[derive(Clone, Debug, PartialEq)]
pub struct Store {
    pub index: usize,
    pub path: [u16; 6],
    pub depth: u16,
    pub kind: &'static str,
    pub loc: Option<Loc>,
    /// Its space's clip, and the origin the block gives it now.
    pub clip: R,
    pub origin: (i32, i32),
    /// The word scrolling it (`0xff`: none) and that word's value.
    pub scroll: Option<u32>,
    pub cap: u32,
    pub lo: u32,
    pub hi: u32,
    pub n: u32,
    pub geo: Option<(i32, i32, i32)>,
    pub seen: u32,
    pub moved: Vec<(&'static str, u32)>,
    pub moved_ok: bool,
    pub cols: Vec<Col>,
    pub records: usize,
    pub pages: usize,
    pub bytes: usize,
    pub subs: Vec<usize>,
}
impl Store {
    pub(crate) fn json(&self) -> J {
        let path: Vec<J> = self.path[..self.depth as usize]
            .iter()
            .map(|&q| J::Int(q as i64))
            .collect();
        J::obj()
            .set("index", self.index)
            .set("path", path)
            .set("kind", self.kind)
            .set("loc", self.loc.map_or(J::Null, |l| l.json()))
            .set("clip", self.clip.json())
            .set(
                "origin",
                vec![J::Int(self.origin.0 as i64), J::Int(self.origin.1 as i64)],
            )
            .set("scroll", self.scroll.map_or(J::Null, |w| J::Int(w as i64)))
            .set("cap", self.cap)
            .set(
                "window",
                vec![J::Int(self.lo as i64), J::Int(self.hi as i64)],
            )
            .set("n", self.n)
            .set(
                "geo",
                self.geo.map_or(J::Null, |(x, y, h)| {
                    J::obj().set("x", x).set("y", y).set("row_h", h)
                }),
            )
            .set("seen", self.seen)
            .set(
                "moved",
                self.moved
                    .iter()
                    .map(|&(op, at)| J::obj().set("op", op).set("at", at))
                    .collect::<Vec<_>>(),
            )
            .set("moved_ok", self.moved_ok)
            .set("cols", self.cols.iter().map(Col::json).collect::<Vec<_>>())
            .set("records", self.records)
            .set("pages", self.pages)
            .set("bytes", self.bytes)
            .set(
                "subs",
                self.subs
                    .iter()
                    .map(|&s| J::Int(s as i64))
                    .collect::<Vec<_>>(),
            )
    }
}

/// A list of the view, as reflection and the store describe it.
#[derive(Clone, Debug, PartialEq)]
pub struct List {
    pub store: usize,
    pub path: [u16; 6],
    pub depth: u16,
    pub loc: Option<Loc>,
    pub n: u32,
    pub row_h: i32,
    pub x: i32,
    pub y: i32,
    pub shape: Vec<R>,
    pub lo: u32,
    pub hi: u32,
    pub cap: u32,
    pub seen: u32,
    pub extent: i64,
    pub scroll: Option<u32>,
}
impl List {
    pub(crate) fn json(&self) -> J {
        let path: Vec<J> = self.path[..self.depth as usize]
            .iter()
            .map(|&q| J::Int(q as i64))
            .collect();
        J::obj()
            .set("store", self.store)
            .set("path", path)
            .set("loc", self.loc.map_or(J::Null, |l| l.json()))
            .set("n", self.n)
            .set(
                "row",
                J::obj()
                    .set("x", self.x)
                    .set("y", self.y)
                    .set("h", self.row_h),
            )
            .set("shape", self.shape.iter().map(R::json).collect::<Vec<_>>())
            .set(
                "window",
                vec![J::Int(self.lo as i64), J::Int(self.hi as i64)],
            )
            .set("cap", self.cap)
            .set("seen", self.seen)
            .set("extent", self.extent)
            .set("scroll", self.scroll.map_or(J::Null, |w| J::Int(w as i64)))
    }
}

/// One live record.
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub key: Key,
    /// The stable identity: call site + store path + row/pos + world generation (never the paint index).
    pub id: u64,
    /// A stable kind id: the call site and the kind name (the `ty` beside it is display-only).
    pub kind_id: u64,
    pub store: usize,
    pub kind: &'static str,
    /// The record type, written out by the compiler. Display-only: closure types differ per build.
    pub ty: &'static str,
    pub depth: u16,
    pub loc: Loc,
    pub rect: R,
    /// Shifted by the space's origin and the paint's own offset: where the pixels are.
    pub screen: R,
    pub clip: R,
    pub visible: bool,
    pub moving: bool,
    pub offset: (i32, i32),
    /// The handler kinds it hears (`hover`, `click`, `press`, `drag`, `wheel`, `key`, `text`, `focus`,
    /// `focusable`).
    pub handlers: Vec<&'static str>,
    pub handler_ty: Option<&'static str>,
    pub ops: Vec<Prop>,
    /// The block words its type reads.
    pub reads: Vec<u32>,
    /// The first content operand (a label's text, a number), as text.
    pub text: Option<String>,
}
impl Node {
    /// The value of a named operand.
    pub fn prop(&self, name: &str) -> Option<&Prop> {
        self.ops.iter().find(|p| p.name == name)
    }
    pub(crate) fn json(&self) -> J {
        J::obj()
            .set("key", self.key.json())
            .set("id", format!("{:016x}", self.id))
            .set("kind_id", format!("{:016x}", self.kind_id))
            .set("store", self.store)
            .set("kind", self.kind)
            .set("ty", self.ty)
            .set("depth", self.depth)
            .set("loc", self.loc.json())
            .set("rect", self.rect.json())
            .set("screen", self.screen.json())
            .set("clip", self.clip.json())
            .set("visible", self.visible)
            .set("moving", self.moving)
            .set(
                "offset",
                vec![J::Int(self.offset.0 as i64), J::Int(self.offset.1 as i64)],
            )
            .set(
                "handlers",
                self.handlers
                    .iter()
                    .map(|&h| J::Str(h.to_owned()))
                    .collect::<Vec<_>>(),
            )
            .set("handler_ty", self.handler_ty)
            .set("ops", self.ops.iter().map(Prop::json).collect::<Vec<_>>())
            .set(
                "reads",
                self.reads
                    .iter()
                    .map(|&w| J::Int(w as i64))
                    .collect::<Vec<_>>(),
            )
            .set("text", self.text.clone())
    }
}

/// The damage the last committed turn produced, before the painter consumed it.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Damage {
    pub tiles: u32,
    /// Tile rows `lo..hi`, each as hex words (bit i of word w: tile `w * 64 + i`).
    pub rows: Vec<(usize, Vec<u64>)>,
    pub sweeps: Vec<Sweep>,
    /// The marked tiles as rectangles, when there are few enough to list.
    pub rects: Vec<R>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Sweep {
    pub col: u32,
    pub until: u32,
    pub rect: R,
    pub origin: (i32, i32),
}
impl Damage {
    pub(crate) fn json(&self) -> J {
        J::obj()
            .set("tiles", self.tiles)
            .set(
                "rows",
                self.rows
                    .iter()
                    .map(|(y, w)| {
                        J::obj().set("y", *y).set(
                            "words",
                            w.iter()
                                .map(|x| J::Str(format!("{x:016x}")))
                                .collect::<Vec<_>>(),
                        )
                    })
                    .collect::<Vec<_>>(),
            )
            .set(
                "sweeps",
                self.sweeps
                    .iter()
                    .map(|s| {
                        J::obj()
                            .set("col", s.col)
                            .set("until", s.until)
                            .set("rect", s.rect.json())
                            .set(
                                "origin",
                                vec![J::Int(s.origin.0 as i64), J::Int(s.origin.1 as i64)],
                            )
                    })
                    .collect::<Vec<_>>(),
            )
            .set("rects", self.rects.iter().map(R::json).collect::<Vec<_>>())
    }
}

/// What the world and the last commands cost. Timings are the session's wall clock, for a report, never a claim.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Counters {
    pub visited_incidences: usize,
    pub evaluated_columns: usize,
    pub dense_fallbacks: usize,
    pub retables: usize,
    pub revision: u32,
    pub records: usize,
    pub pages: usize,
    pub bytes: usize,
    pub commands: u64,
    pub frames: u64,
    pub last_command: String,
    pub last_settle_us: u64,
    pub last_paint_us: u64,
    /// The block words the last command changed (raw, before hot words are excluded).
    pub changed: Vec<u32>,
}
impl Counters {
    pub(crate) fn json(&self) -> J {
        J::obj()
            .set(
                "delta",
                J::obj()
                    .set("visited_incidences", self.visited_incidences)
                    .set("evaluated_columns", self.evaluated_columns)
                    .set("dense_fallbacks", self.dense_fallbacks),
            )
            .set("retables", self.retables)
            .set("revision", self.revision)
            .set(
                "retained",
                J::obj()
                    .set("records", self.records)
                    .set("pages", self.pages)
                    .set("bytes", self.bytes),
            )
            .set("commands", self.commands)
            .set("frames", self.frames)
            .set(
                "changed",
                self.changed
                    .iter()
                    .map(|&w| J::Int(w as i64))
                    .collect::<Vec<_>>(),
            )
            .set(
                "last",
                J::obj()
                    .set("command", self.last_command.as_str())
                    .set("settle_us", self.last_settle_us)
                    .set("paint_us", self.last_paint_us),
            )
    }
}

/// A block word: its value, its registered name if the app gave one, and how many columns read it.
#[derive(Clone, Debug, PartialEq)]
pub struct Word {
    pub word: u32,
    pub name: Option<&'static str>,
    pub value: u32,
    /// The word is a colour, a number or raw bits (the app's field kind, when registered).
    pub kind: &'static str,
    pub readers: u32,
}
impl Word {
    pub(crate) fn json(&self) -> J {
        J::obj()
            .set("word", self.word)
            .set("name", self.name)
            .set("value", self.value)
            .set("kind", self.kind)
            .set("readers", self.readers)
    }
}

/// A typed state field the application registered for inspection and mutation.
#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub name: &'static str,
    pub kind: &'static str,
    pub value: PropValue,
    pub word: Option<u32>,
    pub writable: bool,
}

/// Where the pointer, press, pick and focus are.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Pointers {
    pub pointer: Option<(i32, i32)>,
    pub pick: Option<Key>,
    pub press: Option<Key>,
    pub focus: Option<Key>,
}

/// The accessibility projection, as `bough-ax` publishes it (settled values: motion is where things rest).
#[derive(Clone, Debug, PartialEq)]
pub struct AxNode {
    pub role: &'static str,
    pub name: String,
    pub value: Option<String>,
    pub bounds: R,
    pub children: Vec<usize>,
    pub parent: Option<usize>,
    pub focusable: bool,
    pub focused: bool,
    pub selected: Option<bool>,
    pub row_index: Option<u32>,
    pub row_count: Option<u32>,
    pub col_index: Option<u32>,
    pub col_count: Option<u32>,
    pub live: bool,
    pub actions: Vec<&'static str>,
    pub loc: Option<Loc>,
    /// The record's place word, when the node is one (`World::at_place` maps it back to a node).
    pub place: Option<u32>,
}
impl AxNode {
    pub(crate) fn json(&self) -> J {
        J::obj()
            .set("role", self.role)
            .set("name", self.name.as_str())
            .set("value", self.value.clone())
            .set("bounds", self.bounds.json())
            .set(
                "children",
                self.children
                    .iter()
                    .map(|&c| J::Int(c as i64))
                    .collect::<Vec<_>>(),
            )
            .set("parent", self.parent.map_or(J::Null, |p| J::Int(p as i64)))
            .set("focusable", self.focusable)
            .set("focused", self.focused)
            .set("selected", self.selected)
            .set("row_index", self.row_index)
            .set("row_count", self.row_count)
            .set("col_index", self.col_index)
            .set("col_count", self.col_count)
            .set("live", self.live)
            .set(
                "actions",
                self.actions
                    .iter()
                    .map(|&a| J::Str(a.to_owned()))
                    .collect::<Vec<_>>(),
            )
            .set("loc", self.loc.map_or(J::Null, |l| l.json()))
            .set("place", self.place.map_or(J::Null, |p| J::Int(p as i64)))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Ax {
    pub root: Option<usize>,
    pub focus: Option<usize>,
    pub nodes: Vec<AxNode>,
    /// Nodes the last projection touched (0: nothing had moved).
    pub projected: usize,
}
impl Ax {
    pub(crate) fn json(&self) -> J {
        J::obj()
            .set("root", self.root.map_or(J::Null, |r| J::Int(r as i64)))
            .set("focus", self.focus.map_or(J::Null, |f| J::Int(f as i64)))
            .set("projected", self.projected)
            .set(
                "nodes",
                self.nodes.iter().map(AxNode::json).collect::<Vec<_>>(),
            )
    }
}

/// The application, as the session was told it.
#[derive(Clone, Debug, PartialEq)]
pub struct AppInfo {
    pub name: String,
    pub view: &'static str,
    /// The host's world generation (see `Program::generation`): part of a node id, not of the paint.
    pub generation: u64,
    pub world: u64,
    pub w: i32,
    pub h: i32,
    pub scale: f32,
    pub revision: u32,
    pub input_revision: u64,
    pub shape: u64,
}
impl AppInfo {
    pub(crate) fn json(&self) -> J {
        J::obj()
            .set("name", self.name.as_str())
            .set("view", self.view)
            .set("generation", self.generation)
            .set("world", self.world)
            .set("w", self.w)
            .set("h", self.h)
            .set("scale", self.scale)
            .set("revision", self.revision)
            .set("input_revision", self.input_revision)
            .set("shape", self.shape)
    }
}

/// One snapshot of one world.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Snapshot {
    pub schema: u32,
    pub app: AppInfo,
    pub stores: Vec<Store>,
    pub lists: Vec<List>,
    pub nodes: Vec<Node>,
    pub damage: Damage,
    pub counters: Counters,
    pub words: Vec<Word>,
    pub fields: Vec<Field>,
    pub pointers: Pointers,
    pub ax: Option<Ax>,
}

/// Live snapshot members excluded from goldens and current replay description digests.
/// Paths start at the root. `*` visits each array element.
/// All other fields participate, including `app.generation` (the host remount epoch)
/// and `lists.*.shape` (row geometry).
pub const VOLATILE_SNAPSHOT_FIELDS: &[(&[&str], &[&str])] = &[
    (&[], &["counters", "damage"]),
    (&["app"], &["world", "revision", "input_revision", "shape"]),
    (&["ax"], &["projected"]),
    (
        &["stores", "*"],
        &[
            "cap", "seen", "moved", "moved_ok", "records", "pages", "bytes",
        ],
    ),
    (
        &["stores", "*", "cols", "*"],
        &["pages", "nrec", "records", "bytes"],
    ),
    (&["lists", "*"], &["cap", "seen"]),
    (&["nodes", "*", "key"], &["col", "rec"]),
    (&["pointers", "pick"], &["col", "rec"]),
    (&["pointers", "press"], &["col", "rec"]),
    (&["pointers", "focus"], &["col", "rec"]),
    (&["words", "*"], &["readers"]),
];

fn omit_fields(value: &mut J, path: &[&str], fields: &[&str]) {
    match (value, path) {
        (J::Obj(members), []) => members.retain(|(key, _)| !fields.contains(&key.as_str())),
        (J::Arr(items), ["*", rest @ ..]) => {
            for item in items {
                omit_fields(item, rest, fields);
            }
        }
        (J::Obj(members), [key, rest @ ..]) => {
            if let Some((_, child)) = members.iter_mut().find(|(name, _)| name == key) {
                omit_fields(child, rest, fields);
            }
        }
        _ => {}
    }
}

fn description_hash(json: &str) -> u64 {
    json.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, x| {
        (h ^ x as u64).wrapping_mul(0x100_0000_01b3)
    })
}

impl Default for AppInfo {
    fn default() -> Self {
        AppInfo {
            name: String::new(),
            generation: 0,
            view: "",
            world: 0,
            w: 0,
            h: 0,
            scale: 1.0,
            revision: 0,
            input_revision: 0,
            shape: 0,
        }
    }
}

impl Snapshot {
    pub fn new() -> Snapshot {
        Snapshot {
            schema: SCHEMA_VERSION,
            ..Snapshot::default()
        }
    }
    #[allow(unused_mut)]
    pub fn to_json(&self) -> J {
        let mut o = J::obj()
            .set("schema", self.schema as i64)
            .set("app", self.app.json())
            .set(
                "stores",
                self.stores.iter().map(Store::json).collect::<Vec<_>>(),
            )
            .set(
                "lists",
                self.lists.iter().map(List::json).collect::<Vec<_>>(),
            )
            .set(
                "nodes",
                self.nodes.iter().map(Node::json).collect::<Vec<_>>(),
            )
            .set("damage", self.damage.json())
            .set("counters", self.counters.json())
            .set(
                "words",
                self.words.iter().map(Word::json).collect::<Vec<_>>(),
            )
            .set(
                "fields",
                self.fields
                    .iter()
                    .map(|f| {
                        J::obj()
                            .set("name", f.name)
                            .set("kind", f.kind)
                            .set("value", f.value.json())
                            .set("word", f.word.map_or(J::Null, |w| J::Int(w as i64)))
                            .set("writable", f.writable)
                    })
                    .collect::<Vec<_>>(),
            )
            .set(
                "pointers",
                J::obj()
                    .set(
                        "pointer",
                        self.pointers.pointer.map_or(J::Null, |(x, y)| {
                            vec![J::Int(x as i64), J::Int(y as i64)].into()
                        }),
                    )
                    .set("pick", self.pointers.pick.map_or(J::Null, |k| k.json()))
                    .set("press", self.pointers.press.map_or(J::Null, |k| k.json()))
                    .set("focus", self.pointers.focus.map_or(J::Null, |k| k.json())),
            )
            .set("ax", self.ax.as_ref().map_or(J::Null, Ax::json));
        #[cfg(feature = "mutant_schema")]
        if let J::Obj(m) = &mut o {
            m.retain(|(k, _)| k != "nodes");
        }
        o
    }
    pub fn json_string(&self) -> String {
        json::to_string(&self.to_json())
    }
    /// The serialized description contract used by goldens and current replay logs.
    pub fn stable_json(&self) -> String {
        json::to_string(&self.digest_json())
    }
    /// The live description with the fields in [`VOLATILE_SNAPSHOT_FIELDS`] omitted.
    pub fn digest_json(&self) -> J {
        let mut j = self.to_json();
        for &(path, fields) in VOLATILE_SNAPSHOT_FIELDS {
            omit_fields(&mut j, path, fields);
        }
        j
    }
    /// Digest of the description contract.
    pub fn digest(&self) -> u64 {
        description_hash(&self.stable_json())
    }
    /// The original description projection for version 1 recordings.
    pub(crate) fn legacy_digest(&self) -> u64 {
        let mut j = self.to_json();
        omit_fields(&mut j, &[], &["counters", "damage"]);
        omit_fields(&mut j, &["app"], &["world"]);
        omit_fields(&mut j, &["ax"], &["projected"]);
        description_hash(&json::to_string(&j))
    }
    pub fn node(&self, i: usize) -> Option<&Node> {
        self.nodes.get(i)
    }
    /// The nodes that read block word `w`.
    pub fn readers(&self, w: u32) -> Vec<&Node> {
        self.nodes.iter().filter(|n| n.reads.contains(&w)).collect()
    }
    /// The first node whose key is `k`.
    pub fn find_key(&self, k: &Key) -> Option<&Node> {
        self.nodes.iter().find(|n| n.key == *k)
    }
    /// The first node whose key has the same place (identity without the current materialisation).
    pub fn find_place(&self, depth: u16, path: [u16; 6], row: u32, pos: u32) -> Option<&Node> {
        self.nodes
            .iter()
            .find(|n| n.key.place() == (depth, path, row, pos))
    }
    pub fn store(&self, index: usize) -> Option<&Store> {
        self.stores.get(index)
    }
    /// The chain of stores from the root to `store`.
    pub fn store_chain(&self, store: usize) -> Vec<usize> {
        let mut v = vec![store];
        let mut cur = store;
        while let Some(parent) = self.stores.iter().position(|s| s.subs.contains(&cur)) {
            v.push(parent);
            cur = parent;
        }
        v.reverse();
        v
    }
}
