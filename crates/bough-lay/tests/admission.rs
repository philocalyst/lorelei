//! Soundness review of 2026-11-21, S06: `Shape` is a safe trait and `Nd` a public word array, so the layout
//! must not trust an element's node stream. Both probes read an unwritten `Spec` at 71ee3be (Miri: "using
//! uninitialized data"); they are refused now.
use bough_expr::U;
use bough_lay::shape::{K_BOX, Nd, Sink, place};
use bough_lay::{Hk, Shape, Spec};

struct Empty;
impl Shape for Empty {
    const LEAVES: usize = 0;
    const WRAPS: bool = false;
    const VARIES: bool = false;
    const HK: Hk = Hk::Fixed;
    fn emit<'a, S: Sink<'a>>(&'a self, _: &mut S) {}
}
/// A box that says it has three children and emits none of them.
struct Orphans;
impl Shape for Orphans {
    const LEAVES: usize = 1;
    const WRAPS: bool = false;
    const VARIES: bool = false;
    const HK: Hk = Hk::Fixed;
    fn emit<'a, S: Sink<'a>>(&'a self, s: &mut S) {
        s.node(Nd::pack(
            &Spec::new(false),
            K_BOX,
            true,
            3,
            0.0,
            0.0,
            [0; 2],
        ));
    }
}
#[test]
#[should_panic(expected = "at least one layout node")]
fn an_element_with_no_nodes_is_refused() {
    let mut out = [[0i32; 4]; 1];
    place(&Empty, 1.0, &U::default(), 100.0, &mut out[..]);
}
#[test]
#[should_panic(expected = "more children than its element emitted")]
fn a_box_naming_missing_children_is_refused() {
    let mut out = [[0i32; 4]; 1];
    place(&Orphans, 1.0, &U::default(), 100.0, &mut out[..]);
}
