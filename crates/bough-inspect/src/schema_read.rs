//! Reading a snapshot back. A description is data: a tool can record one, write it to a file and read it again
//! (the overlay over a recorded tree, an offline analysis), and the round-trip test holds the writer and the
//! reader to each other.
//!
//! Source locations, kinds, type names and stage names are `&'static str` in the live description (they come from
//! the compiler). A parsed one interns its strings for the life of the process, as the element layer's `text`
//! does: a snapshot is read a few times, never in a loop.
use crate::json::Value as J;
use crate::schema::*;

fn stat(s: &str) -> &'static str {
    Box::leak(s.to_owned().into_boxed_str())
}
fn s(v: &J, k: &str) -> Result<String, String> {
    v.get(k)
        .and_then(J::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("missing string {k}"))
}
fn opt_s(v: &J, k: &str) -> Option<&'static str> {
    v.get(k).and_then(J::as_str).map(stat)
}
fn i(v: &J, k: &str) -> Result<i64, String> {
    v.get(k)
        .and_then(J::as_i64)
        .ok_or_else(|| format!("missing integer {k}"))
}
fn u(v: &J, k: &str) -> Result<u32, String> {
    i(v, k).map(|x| x as u32)
}
fn f(v: &J, k: &str) -> Result<f32, String> {
    v.get(k)
        .and_then(J::as_f32)
        .ok_or_else(|| format!("missing number {k}"))
}
fn b(v: &J, k: &str) -> bool {
    v.get(k).and_then(J::as_bool).unwrap_or(false)
}
fn arr<'a>(v: &'a J, k: &str) -> &'a [J] {
    v.get(k).map(J::arr).unwrap_or(&[])
}

impl R {
    pub fn from_json(v: &J) -> Result<R, String> {
        let (x, y, w, h) = (
            i(v, "x")? as i32,
            i(v, "y")? as i32,
            i(v, "w")? as i32,
            i(v, "h")? as i32,
        );
        Ok(R {
            x0: x,
            y0: y,
            x1: x + w,
            y1: y + h,
        })
    }
}
impl Loc {
    pub fn from_json(v: &J) -> Result<Loc, String> {
        Ok(Loc {
            file: stat(&s(v, "file")?),
            line: u(v, "line")?,
            col: u(v, "col")?,
        })
    }
}
fn opt_loc(v: &J, k: &str) -> Result<Option<Loc>, String> {
    match v.get(k) {
        Some(x) if !x.is_null() => Ok(Some(Loc::from_json(x)?)),
        _ => Ok(None),
    }
}
impl Key {
    pub fn from_json(v: &J) -> Result<Key, String> {
        let path = arr(v, "path");
        let mut at = [0u16; 6];
        for (k, p) in path.iter().enumerate().take(6) {
            at[k] = p.as_i64().unwrap_or(0) as u16;
        }
        Ok(Key {
            path: at,
            depth: path.len() as u16,
            row: u(v, "row")?,
            pos: u(v, "pos")?,
            col: u(v, "col").unwrap_or(0),
            rec: u(v, "rec").unwrap_or(0),
        })
    }
}
impl PropValue {
    pub fn read_json(v: &J) -> Result<PropValue, String> {
        let kind = v.get("kind").and_then(J::as_str).unwrap_or("none");
        Ok(match kind {
            "none" => PropValue::None,
            "color" => PropValue::Color(u(v, "rgba")?),
            "num" => PropValue::Num {
                int: i(v, "int")? as i32,
                decimals: u(v, "decimals")? as u8,
                text: s(v, "text").unwrap_or_default(),
            },
            "text" => PropValue::Text(s(v, "text")?),
            "pair" => PropValue::Pair(i(v, "x")? as i32, i(v, "y")? as i32),
            "bool" => PropValue::Bool(b(v, "value")),
            "float" => PropValue::Float(f(v, "value")?),
            "int" => PropValue::Int(i(v, "value")? as i32),
            other => return Err(format!("unknown value kind {other:?}")),
        })
    }
}
fn reads(v: &J) -> Vec<u32> {
    arr(v, "reads").iter().filter_map(J::as_u32).collect()
}
impl Prop {
    fn from_json(v: &J) -> Result<Prop, String> {
        Ok(Prop {
            name: stat(&s(v, "name")?),
            word: u(v, "word")?,
            words: u(v, "words")?,
            value: PropValue::read_json(v.get("value").ok_or("prop without a value")?)?,
            stage: stat(&s(v, "stage")?),
            expr: stat(&s(v, "expr")?),
            reads: reads(v),
        })
    }
}
impl Col {
    fn from_json(v: &J) -> Result<Col, String> {
        Ok(Col {
            index: u(v, "index")?,
            kind: stat(&s(v, "kind")?),
            ty: stat(&s(v, "ty")?),
            stage: stat(&s(v, "stage")?),
            reads: reads(v),
            moves: b(v, "moves"),
            timed: b(v, "timed"),
            npages: u(v, "pages")?,
            nrec: u(v, "nrec")?,
            sites: arr(v, "sites").iter().filter_map(J::as_u32).collect(),
            records: i(v, "records")? as usize,
            bytes: i(v, "bytes")? as usize,
        })
    }
}
impl Store {
    fn from_json(v: &J) -> Result<Store, String> {
        let mut path = [0u16; 6];
        let pv = arr(v, "path");
        for (k, p) in pv.iter().enumerate().take(6) {
            path[k] = p.as_i64().unwrap_or(0) as u16;
        }
        let win = arr(v, "window");
        let org = arr(v, "origin");
        let geo = v.get("geo").filter(|g| !g.is_null());
        let mut moved = Vec::new();
        for m in arr(v, "moved") {
            moved.push((
                stat(m.get("op").and_then(J::as_str).unwrap_or("update")),
                u(m, "at")?,
            ));
        }
        Ok(Store {
            index: i(v, "index")? as usize,
            path,
            depth: pv.len() as u16,
            kind: stat(&s(v, "kind")?),
            loc: opt_loc(v, "loc")?,
            clip: R::from_json(v.get("clip").ok_or("store clip")?)?,
            origin: (
                org.first().and_then(J::as_i64).unwrap_or(0) as i32,
                org.get(1).and_then(J::as_i64).unwrap_or(0) as i32,
            ),
            scroll: v.get("scroll").and_then(J::as_u32),
            cap: u(v, "cap")?,
            lo: u32::try_from(win.first().and_then(J::as_i64).unwrap_or(0)).unwrap_or(0),
            hi: u32::try_from(win.get(1).and_then(J::as_i64).unwrap_or(0)).unwrap_or(0),
            n: u(v, "n")?,
            geo: geo.map(|g| {
                (
                    i(g, "x").unwrap_or(0) as i32,
                    i(g, "y").unwrap_or(0) as i32,
                    i(g, "row_h").unwrap_or(0) as i32,
                )
            }),
            seen: u(v, "seen")?,
            moved,
            moved_ok: b(v, "moved_ok"),
            cols: arr(v, "cols")
                .iter()
                .map(Col::from_json)
                .collect::<Result<_, _>>()?,
            records: i(v, "records")? as usize,
            pages: i(v, "pages")? as usize,
            bytes: i(v, "bytes")? as usize,
            subs: arr(v, "subs").iter().filter_map(J::as_usize).collect(),
        })
    }
}
impl List {
    fn from_json(v: &J) -> Result<List, String> {
        let mut path = [0u16; 6];
        let pv = arr(v, "path");
        for (k, p) in pv.iter().enumerate().take(6) {
            path[k] = p.as_i64().unwrap_or(0) as u16;
        }
        let win = arr(v, "window");
        let row = v.get("row").ok_or("list row")?;
        Ok(List {
            store: i(v, "store")? as usize,
            path,
            depth: pv.len() as u16,
            loc: opt_loc(v, "loc")?,
            n: u(v, "n")?,
            row_h: i(row, "h")? as i32,
            x: i(row, "x")? as i32,
            y: i(row, "y")? as i32,
            shape: arr(v, "shape")
                .iter()
                .map(R::from_json)
                .collect::<Result<_, _>>()?,
            lo: u32::try_from(win.first().and_then(J::as_i64).unwrap_or(0)).unwrap_or(0),
            hi: u32::try_from(win.get(1).and_then(J::as_i64).unwrap_or(0)).unwrap_or(0),
            cap: u(v, "cap")?,
            seen: u(v, "seen")?,
            extent: i(v, "extent")?,
            scroll: v.get("scroll").and_then(J::as_u32),
        })
    }
}
impl Node {
    fn from_json(v: &J) -> Result<Node, String> {
        let off = arr(v, "offset");
        Ok(Node {
            key: Key::from_json(v.get("key").ok_or("node key")?)?,
            store: i(v, "store")? as usize,
            kind: stat(&s(v, "kind")?),
            ty: stat(&s(v, "ty")?),
            depth: u(v, "depth")? as u16,
            loc: Loc::from_json(v.get("loc").ok_or("node loc")?)?,
            rect: R::from_json(v.get("rect").ok_or("node rect")?)?,
            screen: R::from_json(v.get("screen").ok_or("node screen")?)?,
            clip: R::from_json(v.get("clip").ok_or("node clip")?)?,
            visible: b(v, "visible"),
            moving: b(v, "moving"),
            offset: (
                off.first().and_then(J::as_i64).unwrap_or(0) as i32,
                off.get(1).and_then(J::as_i64).unwrap_or(0) as i32,
            ),
            handlers: arr(v, "handlers")
                .iter()
                .filter_map(|h| h.as_str().map(stat))
                .collect(),
            handler_ty: opt_s(v, "handler_ty"),
            ops: arr(v, "ops")
                .iter()
                .map(Prop::from_json)
                .collect::<Result<_, _>>()?,
            reads: reads(v),
            text: v.get("text").and_then(J::as_str).map(str::to_owned),
        })
    }
}
impl Damage {
    fn from_json(v: &J) -> Result<Damage, String> {
        let mut rows = Vec::new();
        for r in arr(v, "rows") {
            rows.push((
                i(r, "y")? as usize,
                arr(r, "words")
                    .iter()
                    .filter_map(|w| w.as_str().and_then(|x| u64::from_str_radix(x, 16).ok()))
                    .collect(),
            ));
        }
        let mut sweeps = Vec::new();
        for x in arr(v, "sweeps") {
            let o = arr(x, "origin");
            sweeps.push(Sweep {
                col: u(x, "col")?,
                until: u(x, "until")?,
                rect: R::from_json(x.get("rect").ok_or("sweep rect")?)?,
                origin: (
                    o.first().and_then(J::as_i64).unwrap_or(0) as i32,
                    o.get(1).and_then(J::as_i64).unwrap_or(0) as i32,
                ),
            });
        }
        Ok(Damage {
            tiles: u(v, "tiles")?,
            rows,
            sweeps,
            rects: arr(v, "rects")
                .iter()
                .map(R::from_json)
                .collect::<Result<_, _>>()?,
        })
    }
}
impl Counters {
    fn from_json(v: &J) -> Result<Counters, String> {
        let d = v.get("delta").ok_or("counters delta")?;
        let r = v.get("retained").ok_or("counters retained")?;
        let last = v.get("last").cloned().unwrap_or(J::Null);
        Ok(Counters {
            visited_incidences: i(d, "visited_incidences")? as usize,
            evaluated_columns: i(d, "evaluated_columns")? as usize,
            dense_fallbacks: i(d, "dense_fallbacks")? as usize,
            retables: i(v, "retables")? as usize,
            revision: u(v, "revision")?,
            records: i(r, "records")? as usize,
            pages: i(r, "pages")? as usize,
            bytes: i(r, "bytes")? as usize,
            commands: v.get("commands").and_then(J::as_i64).unwrap_or(0) as u64,
            frames: v.get("frames").and_then(J::as_i64).unwrap_or(0) as u64,
            last_command: last
                .get("command")
                .and_then(J::as_str)
                .unwrap_or("")
                .to_owned(),
            last_settle_us: last.get("settle_us").and_then(J::as_i64).unwrap_or(0) as u64,
            last_paint_us: last.get("paint_us").and_then(J::as_i64).unwrap_or(0) as u64,
            changed: arr(v, "changed").iter().filter_map(J::as_u32).collect(),
        })
    }
}
impl AppInfo {
    fn from_json(v: &J) -> Result<AppInfo, String> {
        Ok(AppInfo {
            name: s(v, "name")?,
            view: stat(&s(v, "view")?),
            world: v.get("world").and_then(J::as_i64).unwrap_or(0) as u64,
            w: i(v, "w")? as i32,
            h: i(v, "h")? as i32,
            scale: f(v, "scale")?,
            revision: u(v, "revision")?,
            input_revision: v.get("input_revision").and_then(J::as_i64).unwrap_or(0) as u64,
            shape: v.get("shape").and_then(J::as_i64).unwrap_or(0) as u64,
        })
    }
}
impl AxNode {
    fn from_json(v: &J) -> Result<AxNode, String> {
        Ok(AxNode {
            role: stat(&s(v, "role")?),
            name: s(v, "name")?,
            value: v.get("value").and_then(J::as_str).map(str::to_owned),
            bounds: R::from_json(v.get("bounds").ok_or("ax bounds")?)?,
            children: arr(v, "children").iter().filter_map(J::as_usize).collect(),
            parent: v.get("parent").and_then(J::as_usize),
            focusable: b(v, "focusable"),
            focused: b(v, "focused"),
            selected: v.get("selected").and_then(J::as_bool),
            row_index: v.get("row_index").and_then(J::as_u32),
            row_count: v.get("row_count").and_then(J::as_u32),
            col_index: v.get("col_index").and_then(J::as_u32),
            col_count: v.get("col_count").and_then(J::as_u32),
            live: b(v, "live"),
            actions: arr(v, "actions")
                .iter()
                .filter_map(|a| a.as_str().map(stat))
                .collect(),
            loc: opt_loc(v, "loc")?,
            place: v.get("place").and_then(J::as_u32),
        })
    }
}
impl Ax {
    fn from_json(v: &J) -> Result<Ax, String> {
        Ok(Ax {
            root: v.get("root").and_then(J::as_usize),
            focus: v.get("focus").and_then(J::as_usize),
            nodes: arr(v, "nodes")
                .iter()
                .map(AxNode::from_json)
                .collect::<Result<_, _>>()?,
            projected: v.get("projected").and_then(J::as_i64).unwrap_or(0) as usize,
        })
    }
}

impl Snapshot {
    /// Read one back. Refuses another schema version, and any member it needs but does not find.
    pub fn from_json(v: &J) -> Result<Snapshot, String> {
        let schema = u(v, "schema")?;
        if schema != SCHEMA_VERSION {
            return Err(format!(
                "snapshot is schema {schema}, this build reads {SCHEMA_VERSION}"
            ));
        }
        let fields = arr(v, "fields")
            .iter()
            .map(|f| {
                Ok(Field {
                    name: stat(&s(f, "name")?),
                    kind: stat(&s(f, "kind")?),
                    value: PropValue::read_json(f.get("value").ok_or("field value")?)?,
                    word: f.get("word").and_then(J::as_u32),
                    writable: b(f, "writable"),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let p = v.get("pointers").cloned().unwrap_or(J::Null);
        let key = |k: &str| -> Result<Option<Key>, String> {
            match p.get(k) {
                Some(x) if !x.is_null() => Ok(Some(Key::from_json(x)?)),
                _ => Ok(None),
            }
        };
        let ptr = arr(&p, "pointer");
        Ok(Snapshot {
            schema,
            app: AppInfo::from_json(v.get("app").ok_or("app")?)?,
            stores: arr(v, "stores")
                .iter()
                .map(Store::from_json)
                .collect::<Result<_, _>>()?,
            lists: arr(v, "lists")
                .iter()
                .map(List::from_json)
                .collect::<Result<_, _>>()?,
            nodes: arr(v, "nodes")
                .iter()
                .map(Node::from_json)
                .collect::<Result<_, _>>()?,
            damage: Damage::from_json(v.get("damage").ok_or("damage")?)?,
            counters: Counters::from_json(v.get("counters").ok_or("counters")?)?,
            words: arr(v, "words")
                .iter()
                .map(|w| {
                    Ok(Word {
                        word: u(w, "word")?,
                        name: opt_s(w, "name"),
                        value: u(w, "value")?,
                        kind: stat(&s(w, "kind")?),
                        readers: u(w, "readers")?,
                    })
                })
                .collect::<Result<_, String>>()?,
            fields,
            pointers: Pointers {
                pointer: ptr.len().eq(&2).then(|| {
                    (
                        ptr[0].as_i64().unwrap_or(0) as i32,
                        ptr[1].as_i64().unwrap_or(0) as i32,
                    )
                }),
                pick: key("pick")?,
                press: key("press")?,
                focus: key("focus")?,
            },
            ax: match v.get("ax") {
                Some(x) if !x.is_null() => Some(Ax::from_json(x)?),
                _ => None,
            },
        })
    }
}
