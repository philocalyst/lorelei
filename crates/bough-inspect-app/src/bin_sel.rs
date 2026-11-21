use bough_guibench_x::rows::Rows;
use bough_inspect::session::Session;
use bough_inspect_app::Lab;
fn main() {
    let mut s = Session::new(Lab::new(Rows::new(2000), 800, 600, 1));
    let snap = s.snapshot();
    for n in &snap.nodes {
        if n.kind == "text" { println!("{:?} row {:?} y {:?}", n.text, n.key.row, n.screen.y0); }
    }
}
