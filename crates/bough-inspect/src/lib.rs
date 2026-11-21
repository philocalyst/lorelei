//! The inspector over the typed core: a versioned, serialisable description of a running `bough-x` world, a
//! validated mutation path, a local agent/test socket, record and replay, pixel diff, typed selectors for tests,
//! and an overlay written with the core's element layer.
//!
//! * [`schema`]: the wire schema (version 1). One owned [`schema::Snapshot`] with the element tree and its source
//!   locations, typed operands and the staged expressions they came from, the block words behind them, layout,
//!   damage, stores and lists, focus/hover/press, the accessibility projection and counters. [`schema::Snapshot::to_json`]
//!   is canonical, so a schema change is a byte diff in a golden.
//! * [`session`]: [`session::Program`] (what an application exposes) and [`session::Session`] (the commands).
//!   Mutation is a registered typed setter, committed through the application's own settle and read back; a
//!   record's operand is a staged expression of state and is read-only, refused with the reason.
//! * [`rpc`] / [`socket`] / [`client`]: JSON-RPC over a length-framed Unix socket. The server is pumped from the
//!   application's own thread; nothing moves a world across threads.
//! * [`record`]: turns as data, with a replay that compares every description and every pixel on a fresh app.
//! * [`png`] / [`frame`]: screenshots and goldens without a dependency.
//! * [`testing`]: typed selectors and a session wrapper, for three-line tests.
//! * [`ui`]: the in-application overlay (tree, selection, source location, properties, dependencies, damage,
//!   timings), written with `bough_x::el` — the typed core's element layer, the façade's own idiom.
//!
//! Cost when unused: a binary that does not link this crate contains none of it (the crate is a separate
//! artifact and `bough-x` is unchanged).
#![feature(peer_credentials_unix_socket)]

pub mod client;
pub mod damage;
pub mod frame;
pub mod json;
pub mod png;
pub mod record;
pub mod rpc;
pub mod schema;
pub mod schema_read;
pub mod session;
pub mod socket;
pub mod testing;
pub mod ui;

pub use frame::Frame;
pub use schema::{Key, Loc, Node, PropValue, SCHEMA_VERSION, Snapshot};
pub use session::{FieldDef, FieldKind, Input, Outcome, Program, Refusal, Session};
