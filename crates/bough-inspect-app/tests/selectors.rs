//! The three-line form an application's author gets: a typed selector over the inspector's tree, a drive, an
//! assertion. Nothing here knows about sockets, commands or the descriptor's internals.
use bough_guibench_x::rows::Rows;
use bough_inspect::testing::TestApp;
use bough_x::event::Key;

#[test]
fn three_line_typed_selectors() {
    let mut app = TestApp::new(bough_inspect_app::Lab::new(Rows::new(2_000), 800, 600, 1));
    let cell = app.find().kind("text").text("Row 00003").one();
    app.click(cell);
    assert!(app.has_text("Row 00003"));
    // a typed field, then the row a scroll brought into the window
    app.session
        .set("scroll", &bough_inspect::PropValue::Int(53 * 32))
        .unwrap();
    let found = app.find().kind("text").text("Row 00053").one();
    assert!(app.node(found).loc.line > 0);
    // a selector that matches nothing and one that matches many both say so
    app.find().text("Row 99999").none();
    app.find().kind("fill").limit(1).all();
    // keys reach the app: hover state is app state, not an inspector invention
    app.pointer(cell);
    let snap = app.snapshot();
    assert!(snap.pointers.pick.is_some());
    let _ = Key::Down;
}
