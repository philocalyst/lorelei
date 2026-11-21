//! The inspector's view of the lab: the typed state fields each scenario exposes, and the `Program` adapter.
use crate::lab::{Event, Lab, Scenario};
use bough_guibench_x::{rows, search, spinner, table};
use bough_inspect::schema::PropValue;
use bough_inspect::session::{FieldDef, FieldKind, Input, Program, Refusal};
use bough_inspect::Frame;
use bough_span::Spans;
use bough_x::prelude::*;

/// A scenario's registered typed state: the fields an inspector may read and write. The setter is the
/// scenario's own lens, so a write is exactly what a handler would do to the block.
pub trait Fields: Scenario {
    fn fields_def() -> &'static [FieldDef<Self>];
}

impl<V: Fields> Program for Lab<V> {
    type V = V;
    type B = Spans<'static>;
    fn name(&self) -> &'static str {
        <V as Scenario>::NAME
    }
    fn view(&self) -> &V {
        &self.scene.v
    }
    fn view_mut(&mut self) -> &mut V {
        &mut self.scene.v
    }
    fn world(&self) -> &World<V, Spans<'static>> {
        &self.scene.world
    }
    fn world_mut(&mut self) -> &mut World<V, Spans<'static>> {
        &mut self.scene.world
    }
    fn settle(&mut self) {
        Lab::settle(self);
    }
    fn size(&self) -> (i32, i32) {
        Lab::size(self)
    }
    fn shot(&mut self) -> Frame {
        let s = Lab::shot(self);
        Frame {
            w: s.w,
            h: s.h,
            px: s.px,
        }
    }
    fn resize(&mut self, w: i32, h: i32, scale: f32) {
        Lab::resize(self, w, h, scale as i32)
    }
    fn advance(&mut self, ms: u32) {
        Lab::advance(self, ms)
    }
    fn input(&mut self, e: &Input) {
        let e = match e {
            Input::Pointer { x, y } => Event::Pointer { x: *x, y: *y },
            Input::Down { x, y, button } => Event::Down {
                x: *x,
                y: *y,
                button: *button,
            },
            Input::Up { x, y, button } => Event::Up {
                x: *x,
                y: *y,
                button: *button,
            },
            Input::Click { x, y } => Event::Click { x: *x, y: *y },
            Input::Key { key, mods } => Event::Key {
                key: *key,
                mods: *mods,
            },
            Input::Text { text } => Event::Text { text: text.clone() },
            Input::Wheel { x, y, dx, dy } => Event::Wheel {
                x: *x,
                y: *y,
                dx: *dx,
                dy: *dy,
            },
        };
        Lab::input(self, &e)
    }
    fn fields(&self) -> &'static [FieldDef<V>] {
        V::fields_def()
    }
    fn fresh(&self) -> Option<Self> {
        Some(self.rebuild_with(self.scene.v.rebuild()))
    }
}

fn int(v: &PropValue, field: &'static str) -> Result<i32, Refusal> {
    match v {
        PropValue::Int(i) => Ok(*i),
        PropValue::Float(f) if f.fract() == 0.0 => Ok(*f as i32),
        _ => Err(Refusal::Rejected {
            field: field.to_owned(),
            why: "an integer",
        }),
    }
}
fn non_negative(v: &PropValue, field: &'static str) -> Result<i32, Refusal> {
    let n = int(v, field)?;
    if n < 0 {
        return Err(Refusal::Rejected {
            field: field.to_owned(),
            why: "a negative index or offset",
        });
    }
    Ok(n)
}

static ROWS_FIELDS: [FieldDef<rows::Rows>; 2] = [
    FieldDef {
        name: "scroll",
        kind: FieldKind::Int,
        word: Some(rows::Scroll::SLOT as u32),
        get: |v: &rows::Rows| PropValue::Int(v.u.get::<rows::Scroll>()),
        set: Some(|v: &mut rows::Rows, x: &PropValue| {
            v.u.set::<rows::Scroll>(non_negative(x, "scroll")?);
            Ok(())
        }),
    },
    FieldDef {
        name: "hot",
        kind: FieldKind::Int,
        word: Some(rows::Hot::SLOT as u32),
        get: |v: &rows::Rows| PropValue::Int(v.u.get::<rows::Hot>() as i32),
        set: Some(|v: &mut rows::Rows, x: &PropValue| {
            v.u.set::<rows::Hot>(non_negative(x, "hot")? as u32);
            Ok(())
        }),
    },
];
impl Fields for rows::Rows {
    fn fields_def() -> &'static [FieldDef<Self>] {
        &ROWS_FIELDS
    }
}

static TABLE_FIELDS: [FieldDef<table::Table>; 3] = [
    FieldDef {
        name: "scroll",
        kind: FieldKind::Int,
        word: Some(table::Scroll::SLOT as u32),
        get: |v: &table::Table| PropValue::Int(v.u.get::<table::Scroll>()),
        set: Some(|v: &mut table::Table, x: &PropValue| {
            v.u.set::<table::Scroll>(non_negative(x, "scroll")?);
            Ok(())
        }),
    },
    FieldDef {
        name: "selected",
        kind: FieldKind::Int,
        word: Some(table::Sel::SLOT as u32),
        get: |v: &table::Table| PropValue::Int(v.u.get::<table::Sel>() as i32),
        set: Some(|v: &mut table::Table, x: &PropValue| {
            v.u.set::<table::Sel>(non_negative(x, "selected")? as u32);
            Ok(())
        }),
    },
    FieldDef {
        // read-only: the row count is the relation's
        name: "len",
        kind: FieldKind::Int,
        word: Some(table::Len::SLOT as u32),
        get: |v: &table::Table| PropValue::Int(v.u.get::<table::Len>() as i32),
        set: None,
    },
];
impl Fields for table::Table {
    fn fields_def() -> &'static [FieldDef<Self>] {
        &TABLE_FIELDS
    }
}

static SEARCH_FIELDS: [FieldDef<search::Search>; 2] = [
    FieldDef {
        name: "scroll",
        kind: FieldKind::Int,
        word: Some(search::Scroll::SLOT as u32),
        get: |v: &search::Search| PropValue::Int(v.u.get::<search::Scroll>()),
        set: Some(|v: &mut search::Search, x: &PropValue| {
            v.u.set::<search::Scroll>(non_negative(x, "scroll")?);
            Ok(())
        }),
    },
    FieldDef {
        name: "len",
        kind: FieldKind::Int,
        word: Some(search::Len::SLOT as u32),
        get: |v: &search::Search| PropValue::Int(v.u.get::<search::Len>() as i32),
        set: None,
    },
];
impl Fields for search::Search {
    fn fields_def() -> &'static [FieldDef<Self>] {
        &SEARCH_FIELDS
    }
}

static SPINNER_FIELDS: [FieldDef<spinner::Spinner>; 1] = [FieldDef {
    name: "scroll",
    kind: FieldKind::Int,
    word: Some(spinner::Scroll::SLOT as u32),
    get: |v: &spinner::Spinner| PropValue::Int(v.u.get::<spinner::Scroll>()),
    set: Some(|v: &mut spinner::Spinner, x: &PropValue| {
        v.u.set::<spinner::Scroll>(non_negative(x, "scroll")?);
        Ok(())
    }),
}];
impl Fields for spinner::Spinner {
    fn fields_def() -> &'static [FieldDef<Self>] {
        &SPINNER_FIELDS
    }
}
