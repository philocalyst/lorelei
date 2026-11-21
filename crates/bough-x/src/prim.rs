//! The generic record: what the complete-capability path stores (Integration Card 3, decision D2).
//!
//! A typed archetype (`kinds.rs`, `el.rs`) is a paint kind applied to expression types, one column per call site,
//! shaded by a kernel made for it. A description that is data (the façade lowered, `bough::lower`) has no such
//! types: its records are of this one archetype, whose kind is a word of the record. That costs a branch per
//! record while shading and gives up the per-word routing of a typed column (a `Prim` reads no block word: it
//! changes only by being described again), and in return every element an author can write has a record in the
//! same world, the same columns, the same tile damage and the same paint order as the typed ones.
//!
//! Colours are straight-alpha RGBA8 with red in the low byte (the façade's `Color`); a backend composites them
//! premultiplied. Nothing here is forced opaque.
use crate::draw::{Draws, Prims};
use crate::expr::{StageId, U, Words};
use crate::store::{Arch, Backend, ColRef, Rect};

/// A box: fill, corner radius, border.
pub const P_BOX: u32 = 0;
/// A shaped line of text: `a` is the run, `b` its generation, in the backend's run store.
pub const P_TEXT: u32 = 1;
/// An image: `a` is its id in the backend's image store.
pub const P_IMAGE: u32 = 2;

/// One record of the generic archetype. Its rect (the column's rect lanes) is the box, or the ink box of a line.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Prim {
    pub kind: u32,
    /// Fill (box), ink colour (text), tint (image). Straight alpha.
    pub color: u32,
    /// Corner radius in 1/256 device pixels.
    pub radius: u32,
    /// Border width in 1/256 device pixels (0: none) and colour.
    pub border: u32,
    pub border_color: u32,
    pub a: u32,
    pub b: u32,
    /// Opacity of the whole record, 0..=255.
    pub alpha: u32,
    /// Where the content's origin is inside the rect (a line's pen origin and baseline), device pixels.
    pub ox: i32,
    pub oy: i32,
}
// SAFETY: `repr(C)` over ten 32-bit fields: forty bytes, alignment 4, no padding, every bit pattern a value.
unsafe impl Words for Prim {}
const _: () = assert!(size_of::<Prim>() == 40 && align_of::<Prim>() == 4);

impl Arch for Prim {
    const READS: u64 = 0;
    const NAME: &'static str = "prim";
    const STAGE: StageId = StageId::Mount;
    #[inline(always)]
    fn changed(&self, _: &U, _: &U) -> bool {
        false
    }
    #[inline(always)]
    fn paint<B: Backend>(b: &mut B, col: ColRef<'_, Self>, u: &U) {
        b.draws(col, u)
    }
}
impl Draws for Prim {
    #[inline(always)]
    fn draw<P: Prims>(&self, r: Rect, cut: Rect, _: &U, p: &mut P) {
        p.prim(r, cut, self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::Record;

    /// The `Words` claim above: two records are the same record exactly when their fields are equal, and the
    /// words are the fields in order.
    #[test]
    fn a_prim_is_ten_words_compared_as_fields() {
        let a = Prim {
            kind: P_TEXT,
            color: 0xff00_00ff,
            radius: 3,
            border: 4,
            border_color: 5,
            a: 6,
            b: 7,
            alpha: 255,
            ox: -1,
            oy: 9,
        };
        assert!(<Prim as Record>::WORD_LAYOUT);
        assert_eq!(
            a.record_words(),
            &[P_TEXT, 0xff00_00ff, 3, 4, 5, 6, 7, 255, u32::MAX, 9]
        );
        let mut b = a;
        assert!(a.same_record(&b));
        b.oy = 8;
        assert!(!a.same_record(&b));
    }
}
