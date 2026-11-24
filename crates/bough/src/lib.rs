#![feature(type_info, ptr_metadata, rustc_attrs, const_trait_impl, const_cmp, arbitrary_self_types_pointers, f16)]
#![cfg_attr(target_arch = "aarch64", feature(stdarch_neon_f16))]
#![allow(internal_features, incomplete_features)]
#![feature(specialization, impl_trait_in_assoc_type)]
//! bough: the public façade over bough-core (records, damage), bough-text (shaping into the glyph pool),
//! cephal (layout) and bough-metal's fused presenter. One region holds everything the GPU reads
//! (`rt.rs`); a frame's CPU work ends at the tile list (`app.rs`).
//!
//! There is one author surface: [`prelude`] (`designs/reactivity/final`). [`raw`] is the scene-level surface the
//! existing benchmark ports are written in. Everything else that is `pub` is machinery, exposed for the
//! harnesses and tests that hold it to its oracles, and is not an API to write applications against:
//!
//! * [`flat::FlatApp`] drives any prelude view on the flat core (`bough-flat`) and the damage engine
//!   (`bough-damage`), with no type-level dataflow.
//! * `flat::ui`, `style`, `motion` and `pages` are engines (a builder on the flat core's bindings; boxes,
//!   gradients, shadows and blurs as values on the paint lane's flat bank; springs, curves and keyframes;
//!   retained pages and the transitions between them). They are hidden from the documentation: what an
//!   author writes is a prelude port, and a port is lowered onto these.
pub mod app;
pub mod backend;
pub mod boxops;
pub mod changed;
pub mod color;
pub mod kernel_backend;
pub mod deco;
pub mod describe;
pub mod el;
pub mod elem;
pub mod entity;
pub mod flat;
pub mod input;
pub mod inspect;
pub mod lanes;
pub mod lay_deep;
pub mod layout;
pub mod ledger;
pub mod list;
pub mod lower;
pub mod listview;
pub mod ltemplate;
pub mod mlist;
#[doc(hidden)]
pub mod motion;
#[doc(hidden)]
pub mod pages;
pub mod reader;
pub mod reflect;
pub mod rt;
pub mod seal;
pub mod select;
pub mod signal;
pub mod store;
#[doc(hidden)]
pub mod style;
pub mod template;
pub mod text;
pub mod transition;
pub mod typed;
pub mod ui;

pub use bough_text::numeric::NumFmt;
/// The OS counters (instructions, memory) and the frame contract, for harnesses and benchmarks on every platform.
pub use bough_frame as frame;
pub use bough_sys as sys;

/// The application surface (`designs/reactivity/final`): a view is a plain struct whose `view` returns an
/// [`El`](crate::el::El); elements in flow (laid out by cephal), ports that take constants, signal fields and
/// mapped sources, children as tuples, `Option`s and iterators, routes as methods, and `Application::run`.
pub mod prelude {
    pub use crate::changed::Changed;
    pub use crate::color::{rgb, Color};
    pub use crate::el::plug::{IntoProp, Len, Sink};
    pub use crate::el::{block, button, column, div, image, para, row, text, text_input, ChildSeq, El, Hover, Map, Route, Source, View, With};
    pub use crate::elem::each;
    pub use crate::entity::{Entities, Entity};
    pub use crate::input::{Ev, Key, Mods};
    pub use crate::layout::{Align, Justify, Styled};
    pub use crate::list::{List, Slot};
    pub use crate::reader::{Reader, Request};
    pub use crate::signal::Signal;
    pub use crate::transition::{Curve, Ease};
    pub use crate::ui::{Application, Flex, Window};
}

/// The kernel's typed builders in flow (what [`El`](crate::el::El) is walked onto, and what the measured
/// workload scenes are written in): every element a nested generic type over its owner, operands wired with
/// `sig!`. Machinery, like [`raw`]; an application writes [`prelude`].
pub mod builders {
    pub use crate::app::View;
    pub use crate::changed::Changed;
    pub use crate::color::{rgb, Color};
    pub use crate::elem::{each, Element, IntoProp};
    pub use crate::entity::{Entities, Entity};
    pub use crate::list::{List, Slot};
    pub use crate::listview::list;
    pub use crate::reader::{boundary, Reader, Request};
    pub use crate::input::{text_field, tooltip, Interact, Key, Mods};
    pub use crate::layout::{block, column, para, row, Align, Justify, Styled};
    pub use crate::sig;
    pub use crate::signal::Signal;
    pub use crate::transition::{Curve, Ease};
    pub use crate::ui::{button, div, image, text, text_input, Application, Flex, Window};
}

/// The scene-level surface the benchmark ports are written in: absolutely placed quads and text, templates,
/// measured lists, the `App` driver. `use bough::raw::*;`.
pub mod raw {
    pub use crate::transition::{Curve, Ease, Lerp};
    pub use crate::deco::{deco_box, svg_image, Deco, Grad, Shadow, Xf};
    pub use crate::app::{App, Config, View};
    pub use crate::color::{rgb, Color};
    pub use crate::elem::{at, bars, clip, each, quad, text, virtual_list, Element, IntoProp, Mounter, Quad, TextSrc};
    pub use crate::changed::Changed;
    pub use crate::entity::{entity_view, Entities, Entity};
    pub use crate::input::{no_hit, text_field, tooltip, Button, Ev, Interact, Key, Mods, TipStyle};
    pub use crate::ltemplate::{ltemplate, wire_restyle, Decls, LCtx, LHooked, LNode, P};
    pub use crate::template::{anchor, clip_by, offset, port_list, port_list_in, scroll_y, template, wired_clip, Hooked, TNode, T_CLIP, T_FOCUS, T_HOOK};
    pub use crate::lanes::{lanes, Lane};
    pub use crate::list::{List, Slot};
    pub use crate::listview::{index_list, list, list_with};
    pub use crate::reader::{boundary, latest, produce, Reader, Request};
    pub use crate::rt::Caps;
    pub use crate::sig;
    pub use crate::signal::{Signal, Wired};
    pub use bough_text::numeric::NumFmt;
}
