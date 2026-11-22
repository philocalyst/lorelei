use bough_dataflow::prelude::Rect;
use bough_dataflow::soft::Soft;
use bough_dataflow::store::Tiles;

#[test]
fn rectangular_damage_paints_once_and_holes_keep_their_pixels() {
    let mut frame = Soft::new(101, 77);
    let mut damage = Tiles::new(101, 77);
    let all = Rect::new(0, 0, 101, 77);
    damage.mark_all();
    let mut calls = 0;
    frame.repair(&damage, |out| {
        calls += 1;
        out.rect(all, 0x224466);
    });
    assert_eq!(calls, 1);
    assert!(frame.px.iter().all(|&c| c == 0x224466));

    damage.clear();
    damage.mark(Rect::new(0, 0, 32, 32));
    damage.mark(Rect::new(96, 64, 5, 13));
    frame.repair(&damage, |out| out.rect(all, 0x887788));
    for y in 0..77 {
        for x in 0..101 {
            let expected = if (x < 32 && y < 32) || (x >= 96 && y >= 64) {
                0x887788
            } else {
                0x224466
            };
            assert_eq!(frame.px[y * 101 + x], expected, "{x},{y}");
        }
    }
}
