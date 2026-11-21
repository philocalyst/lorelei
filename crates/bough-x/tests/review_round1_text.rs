use bough_face::{Glyphs, Run, key};
use bough_x::store::Rect;
use bough_x::text::TextCx;

#[test]
fn refused_runs_and_missing_faces_emit_no_spans() {
    let mut source = Glyphs::new();
    let (run, _) = source
        .line_with(key(17, b"A"), 17, b"A", |slots, pens| {
            slots.push(1);
            pens.push(0);
            Some(4)
        })
        .unwrap();
    let foreign = Glyphs::new();
    let rect = Rect::new(0, 0, 8, 8);
    let cx = TextCx {
        faces: &[],
        glyphs: &foreign,
    };
    for refused in [Run::default(), run] {
        cx.line(refused, rect, rect, |_, _, _| panic!("refused run drew"));
    }
    let cx = TextCx {
        faces: &[],
        glyphs: &source,
    };
    cx.line(run, rect, rect, |_, _, _| panic!("missing face drew"));
    cx.num(3, 0, 0, rect, rect, |_, _, _| panic!("missing face drew"));
}

#[test]
fn refused_shaper_does_not_overwrite_an_older_run() {
    let mut glyphs = Glyphs::new();
    let (run, _) = glyphs
        .line_with(key(0, b"A"), 0, b"A", |slots, pens| {
            slots.push(1);
            pens.push(10);
            Some(4)
        })
        .unwrap();
    assert!(
        glyphs
            .line_with(key(0, b"B"), 0, b"B", |slots, pens| {
                slots.as_mut_slice().first_mut().map(|first| *first = 2);
                pens.as_mut_slice().first_mut().map(|first| *first = 20);
                None
            })
            .is_none()
    );
    assert_eq!(glyphs.of(run), (&[1][..], &[10][..]));
}

#[test]
fn a_panicking_custom_shaper_leaves_published_runs_intact() {
    let mut glyphs = Glyphs::new();
    let (first, _) = glyphs
        .line_with(1, 0, b"first", |slots, pens| {
            slots.push(7);
            pens.push(12);
            Some(20)
        })
        .unwrap();
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        glyphs.line_with(2, 0, b"retry", |slots, pens| {
            slots.clear();
            pens.clear();
            slots.push(99);
            panic!("test shaper stopped");
        });
    }));
    assert!(failure.is_err());
    assert_eq!(glyphs.of(first), (&[7][..], &[12][..]));
    let (retried, _) = glyphs
        .line_with(2, 0, b"retry", |slots, pens| {
            assert!(slots.is_empty() && pens.is_empty());
            slots.push(9);
            pens.push(17);
            Some(30)
        })
        .unwrap();
    assert_eq!(glyphs.of(retried), (&[9][..], &[17][..]));
    assert_eq!(glyphs.of(first), (&[7][..], &[12][..]));
}
