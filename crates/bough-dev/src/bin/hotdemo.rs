//! A lorelei view whose variable text crosses the hot-call boundary (`bough_dev::hotview`).
//!
//! ```text
//! hotdemo --socket /tmp/hot.sock
//! ```
//!
//! The process serves the inspector socket like any host, so a developer-loop client can read the reflected
//! text, the world's identity and the revision, and can ask it to `hotpatch` a dylib. The patch replaces the
//! text function and the view is re-described in the same world.
use bough_dev::hotview;
use bough_inspect::Frame;
use bough_inspect::session::Session;
use bough_inspect::session::{FieldDef, Input, Program};
use bough_inspect::socket::Server;
use bough_x::el::App;
use bough_x::el::prelude::*;
use bough_x::soft::Soft;
use bough_x::world::World;

struct DemoView;
impl View for DemoView {
    fn view(&self) -> impl Element<Self> + '_ {
        column((
            text(hotview::text())
                .font(12.0, 20.0)
                .color(rgb(0xffffffff)),
            div().w(140.0).h(20.0).bg(rgb(0x224466)),
        ))
        .w(160.0)
        .h(48.0)
        .bg(rgb(0x111111))
    }
}

struct Demo {
    app: App<DemoView, Soft>,
    w: i32,
    h: i32,
    k: f32,
    generation: u64,
}

impl Program for Demo {
    type V = DemoView;
    type B = Soft;
    fn name(&self) -> &'static str {
        "hotdemo"
    }
    fn generation(&self) -> u64 {
        self.generation
    }
    fn view(&self) -> &DemoView {
        &self.app.v
    }
    fn view_mut(&mut self) -> &mut DemoView {
        &mut self.app.v
    }
    fn world(&self) -> &World<DemoView, Soft> {
        &self.app.world
    }
    fn world_mut(&mut self) -> &mut World<DemoView, Soft> {
        &mut self.app.world
    }
    fn settle(&mut self) {
        self.app.settle();
    }
    fn size(&self) -> (i32, i32) {
        (self.w, self.h)
    }
    fn shot(&mut self) -> Frame {
        let mut s = Soft::new(self.w, self.h);
        self.app.world.paint(&mut s);
        Frame {
            w: self.w,
            h: self.h,
            px: s.px,
        }
    }
    fn resize(&mut self, w: i32, h: i32, scale: f32) {
        self.w = w;
        self.h = h;
        self.k = scale;
        self.app = App::new(DemoView, w, h, scale);
        self.generation += 1;
    }
    fn input(&mut self, e: &Input) {
        match *e {
            Input::Pointer { x, y } => {
                self.app.world.pointer(&mut self.app.v, x, y);
                self.app.settle();
            }
            Input::Down { x, y, button } => {
                self.app.world.pointer_down(&mut self.app.v, x, y, button);
                self.app.settle();
            }
            Input::Up { x, y, button } => {
                self.app.world.pointer_up(&mut self.app.v, x, y, button);
                self.app.settle();
            }
            Input::Click { x, y } => {
                self.app.world.click(&mut self.app.v, x, y);
                self.app.settle();
            }
            Input::Key { key, .. } => {
                self.app.key(key);
            }
            _ => {}
        }
    }
    fn fields(&self) -> &'static [FieldDef<DemoView>] {
        &[]
    }
    fn hot_patch(&mut self, path: &std::path::Path) -> Result<(), String> {
        hotview::install(path, hotview::STATE_TOKEN)?;
        #[cfg(not(feature = "mutant_no_redescribe"))]
        {
            // new code, same world: the description is made again from the installed function
            self.app.world.redescribe(&self.app.v);
            self.app.settle();
        }
        Ok(())
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut socket = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--socket" => {
                socket = args.get(i + 1).cloned();
                i += 1;
            }
            other => {
                eprintln!("hotdemo: unknown argument {other:?}");
                std::process::exit(2);
            }
        }
        i += 1;
    }
    let Some(socket) = socket else {
        eprintln!("hotdemo: --socket PATH is required");
        std::process::exit(2);
    };
    let app = App::<DemoView, Soft>::new(DemoView, 200, 60, 1.0);
    let demo = Demo {
        app,
        w: 200,
        h: 60,
        k: 1.0,
        generation: 0,
    };
    let mut session = Session::new(demo);
    let _ = session.shot();
    let mut server = match Server::bind(&socket) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("hotdemo: cannot bind {socket}: {e}");
            std::process::exit(1);
        }
    };
    println!("ready {socket}");
    let result = server.serve(&mut session);
    drop(server);
    if let Err(e) = result {
        eprintln!("hotdemo: {e}");
        std::process::exit(1);
    }
}
