//! Soundness review of 2026-11-21, S10: size arithmetic in safe entry points is checked. At 71ee3be, in a
//! build without overflow checks (`release`), both calls below wrapped past their bound check and wrote
//! outside the region / column.
use bough_mem::{Col, Region};

#[test]
#[should_panic(expected = "region offset overflow")]
fn a_reservation_that_wraps_is_refused() {
    let mut r = Region::heap(1 << 16);
    r.reserve(64, 64);
    r.reserve(usize::MAX - 10, 64);
}

#[test]
#[should_panic(expected = "column length overflow")]
fn an_insert_that_wraps_is_refused() {
    let mut c: Col<u32> = Col::owned("c");
    c.push(1);
    c.insert_n(0, usize::MAX, 0);
}

#[test]
fn ordinary_reservations_still_fit() {
    let mut r = Region::heap(1 << 16);
    assert_eq!(r.reserve(100, 64), 0);
    assert_eq!(r.reserve(100, 64), 128);
    let mut c: Col<u32> = Col::owned("c");
    c.extend_from_slice(&[1, 2, 3]);
    c.insert_n(1, 2, 9);
    assert_eq!(&c[..], &[1, 9, 9, 2, 3]);
}
