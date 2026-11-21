//! `bough-inspect-app`: a real comparison app (bough-guibench-x) on a headless host, with the agent/test socket.
//!
//! ```text
//! bough-inspect-app --scenario rows --rows 10000 --socket /tmp/bl.sock
//! ```
//!
//! With `--socket`, the process mounts the scenario, binds the socket, prints `ready <path>` on stdout and serves
//! until a `shutdown` request or the client leaves. Without it, it paints one frame and prints its digest (the
//! zero-cost build's self-check).
use bough_inspect_app::{Lab, Scenario};
use bough_guibench_x::{rows, search, spinner, table};

struct Args {
    scenario: String,
    rows: usize,
    socket: Option<String>,
    w: i32,
    h: i32,
    scale: i32,
    filter: bool,
}

fn parse() -> Result<Args, String> {
    let mut a = Args {
        scenario: "rows".into(),
        rows: 1_000,
        socket: None,
        w: 800,
        h: 600,
        scale: 1,
        filter: true,
    };
    let mut it = std::env::args().skip(1);
    while let Some(k) = it.next() {
        let mut val = || it.next().ok_or_else(|| format!("{k} needs a value"));
        match k.as_str() {
            "--scenario" => a.scenario = val()?,
            "--rows" => a.rows = val()?.parse().map_err(|_| "rows: a number")?,
            "--socket" => a.socket = Some(val()?),
            "--w" => a.w = val()?.parse().map_err(|_| "w: a number")?,
            "--h" => a.h = val()?.parse().map_err(|_| "h: a number")?,
            "--scale" => a.scale = val()?.parse().map_err(|_| "scale: a number")?,
            "--no-filter" => a.filter = false,
            "--help" | "-h" => {
                println!(
                    "usage: bough-inspect-app [--scenario rows|table|search|spinner] [--rows N] [--socket PATH] [--w W] [--h H] [--scale K] [--no-filter]"
                );
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    Ok(a)
}

fn main() {
    let a = match parse() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("bough-inspect-app: {e}");
            std::process::exit(2);
        }
    };
    match a.scenario.as_str() {
        "rows" => run(rows::Rows::new(a.rows), &a),
        "table" => run(table::Table::new(a.rows, None), &a),
        "search" => run(search::Search::new(a.rows, a.filter), &a),
        "spinner" => run(spinner::Spinner::new(a.rows), &a),
        other => {
            eprintln!("bough-inspect-app: no scenario {other:?}");
            std::process::exit(2);
        }
    }
}

#[cfg(feature = "inspect")]
fn run<V: bough_inspect_app::Fields>(v: V, a: &Args) -> ! {
    use bough_inspect::session::Session;
    use bough_inspect::socket::Server;
    let lab = Lab::new(v, a.w, a.h, a.scale);
    let mut session = Session::new(lab);
    // a first frame, so the world's damage is consumed as a host would after presenting
    let _ = session.shot();
    match &a.socket {
        Some(path) => {
            let mut server = match Server::bind(path) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("bough-inspect-app: cannot bind {path}: {e}");
                    std::process::exit(1);
                }
            };
            println!("ready {path}");
            if let Err(e) = server.serve(&mut session) {
                eprintln!("bough-inspect-app: {e}");
                std::process::exit(1);
            }
            std::process::exit(0);
        }
        None => {
            let f = session.shot();
            println!(
                "{} {}x{} digest {:016x}",
                <V as Scenario>::NAME,
                f.w,
                f.h,
                f.digest()
            );
            std::process::exit(0);
        }
    }
}

#[cfg(not(feature = "inspect"))]
fn run<V: Scenario>(v: V, a: &Args) -> ! {
    let mut lab = Lab::new(v, a.w, a.h, a.scale);
    lab.settle();
    let f = lab.shot();
    println!(
        "{} {}x{} digest {:016x}",
        <V as Scenario>::NAME,
        f.w,
        f.h,
        {
            // FNV-1a, the same digest the inspector uses, without linking it
            f.px.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &p| {
                let mut h = h;
                for b in p.to_le_bytes() {
                    h = (h ^ b as u64).wrapping_mul(0x100_0000_01b3);
                }
                h
            })
        }
    );
    if a.socket.is_some() {
        eprintln!("bough-inspect-app: this build has the inspector off (--no-default-features)");
        std::process::exit(3);
    }
    std::process::exit(0);
}
