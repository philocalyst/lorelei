//! The terminal client: a person or a script reads the tree, picks, sets a field, drives input, takes a
//! screenshot and diffs it. Every subcommand is one inspector request, printed as JSON unless it is a text view.
use bough_inspect::client::Client;
use bough_inspect::json::{self, Value as J};
use bough_inspect::png;
use std::path::Path;

type R = Result<(), String>;

fn call(c: &mut Client, method: &str, p: J) -> Result<J, String> {
    c.request(method, &p)
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("{}: {}", e.code, e.message))
}

pub fn print_tree(v: &J) -> String {
    let app = v.get("app").unwrap_or(&J::Null);
    let mut s = String::new();
    s.push_str(&format!(
        "{}  {}x{} @{}x  rev {}  records {}  damage {} tiles  digest {}\n",
        app.get("name").and_then(J::as_str).unwrap_or("?"),
        app.get("w").and_then(J::as_i64).unwrap_or(0),
        app.get("h").and_then(J::as_i64).unwrap_or(0),
        app.get("scale").and_then(J::as_f64).unwrap_or(1.0),
        app.get("revision").and_then(J::as_i64).unwrap_or(0),
        v.get("counters")
            .and_then(|c| c.get("retained"))
            .and_then(|r| r.get("records"))
            .and_then(J::as_i64)
            .unwrap_or(0),
        v.get("damage")
            .and_then(|d| d.get("tiles"))
            .and_then(J::as_i64)
            .unwrap_or(0),
        v.get("digest").and_then(J::as_str).unwrap_or(""),
    ));
    for n in v.get("nodes").map(J::arr).unwrap_or(&[]) {
        let depth = n.get("depth").and_then(J::as_i64).unwrap_or(0) as usize;
        let kind = n.get("kind").and_then(J::as_str).unwrap_or("");
        let text = n.get("text").and_then(J::as_str).unwrap_or("");
        let loc = n.get("loc").unwrap_or(&J::Null);
        let handlers: Vec<&str> = n
            .get("handlers")
            .map(J::arr)
            .unwrap_or(&[])
            .iter()
            .filter_map(J::as_str)
            .collect();
        let screen = n.get("screen").unwrap_or(&J::Null);
        s.push_str(&format!(
            "{:indent$}{:<7} {}{}  [{}:{}]  {},{} {}x{}{}\n",
            "",
            kind,
            if text.is_empty() {
                String::new()
            } else {
                format!("{text:?} ")
            },
            loc.get("file").and_then(J::as_str).unwrap_or(""),
            loc.get("line").and_then(J::as_i64).unwrap_or(0),
            loc.get("col").and_then(J::as_i64).unwrap_or(0),
            screen.get("x").and_then(J::as_i64).unwrap_or(0),
            screen.get("y").and_then(J::as_i64).unwrap_or(0),
            screen.get("w").and_then(J::as_i64).unwrap_or(0),
            screen.get("h").and_then(J::as_i64).unwrap_or(0),
            if handlers.is_empty() {
                String::new()
            } else {
                format!("  [{}]", handlers.join(","))
            },
            indent = depth * 2,
        ));
    }
    s
}

fn value_of(kind: &str, v: &str) -> Result<J, String> {
    Ok(match kind {
        "int" => J::obj()
            .set("kind", "int")
            .set("value", v.parse::<i64>().map_err(|_| "int")?),
        "float" => J::obj()
            .set("kind", "float")
            .set("value", v.parse::<f64>().map_err(|_| "float")?),
        "bool" => J::obj()
            .set("kind", "bool")
            .set("value", v == "true" || v == "1"),
        "color" => J::obj()
            .set(
                "rgba",
                u32::from_str_radix(v.trim_start_matches("0x").trim_start_matches('#'), 16)
                    .map_err(|_| "color")?,
            )
            .set("kind", "color"),
        "text" => J::obj().set("kind", "text").set("text", v),
        other => return Err(format!("no value kind {other:?}")),
    })
}

pub fn value_text(v: &J) -> String {
    if let Some(t) = v.get("text").and_then(J::as_str) {
        return t.to_owned();
    }
    if let Some(x) = v.get("rgba").and_then(J::as_i64) {
        return format!("#{:08x}", x as u32);
    }
    v.get("value")
        .map(|x| {
            x.as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| json::to_string(x))
        })
        .unwrap_or_default()
}

/// Run `dev inspect <cmd> ...` against `socket`. The first arg is the subcommand.
pub fn run(socket: &Path, args: &[String]) -> R {
    let mut c = Client::open(socket).map_err(|e| format!("{}: {e}", socket.display()))?;
    let cmd = args.first().map(String::as_str).unwrap_or("tree");
    let opts = &args[1..];
    match cmd {
        "tree" => {
            let limit = opts
                .iter()
                .position(|o| o == "--limit")
                .and_then(|i| opts.get(i + 1))
                .and_then(|v| v.parse::<i64>().ok());
            let mut p = J::obj();
            if let Some(l) = limit {
                p.put("limit", l);
            }
            let v = call(&mut c, "tree", p)?;
            if opts.iter().any(|o| o == "--json") {
                println!("{}", json::to_string(&v));
            } else {
                print!("{}", print_tree(&v));
            }
        }
        "node" => {
            let i = opts
                .first()
                .and_then(|x| x.parse::<i64>().ok())
                .ok_or("node <index>")?;
            println!(
                "{}",
                json::to_string(&call(&mut c, "node", J::obj().set("index", i))?)
            );
        }
        "find" => {
            let mut p = J::obj();
            let mut iter = opts.iter();
            while let Some(o) = iter.next() {
                match o.as_str() {
                    "--kind" => p.put("kind", iter.next().ok_or("--kind value")?.as_str()),
                    "--text" => p.put("text", iter.next().ok_or("--text value")?.as_str()),
                    "--ty" => p.put("ty", iter.next().ok_or("--ty value")?.as_str()),
                    "--role" => p.put("role", iter.next().ok_or("--role value")?.as_str()),
                    "--limit" => p.put(
                        "limit",
                        iter.next()
                            .ok_or("--limit value")?
                            .parse::<i64>()
                            .map_err(|_| "--limit")?,
                    ),
                    "--ops" => p.put("ops", true),
                    other => return Err(format!("find: unknown option {other:?}")),
                }
            }
            let v = call(&mut c, "find", p)?;
            if opts.iter().any(|o| o == "--json") {
                println!("{}", json::to_string(&v));
            } else {
                for n in v.get("nodes").map(J::arr).unwrap_or(&[]) {
                    println!(
                        "#{} {:<7} {}  {}:{}",
                        n.get("index").and_then(J::as_i64).unwrap_or(0),
                        n.get("kind").and_then(J::as_str).unwrap_or(""),
                        n.get("text").and_then(J::as_str).unwrap_or(""),
                        n.get("loc")
                            .and_then(|l| l.get("file"))
                            .and_then(J::as_str)
                            .unwrap_or(""),
                        n.get("loc")
                            .and_then(|l| l.get("line"))
                            .and_then(J::as_i64)
                            .unwrap_or(0),
                    );
                }
                println!("{} total", v.get("total").and_then(J::as_i64).unwrap_or(0));
            }
        }
        "pick" => {
            let x = opts
                .first()
                .and_then(|v| v.parse::<i64>().ok())
                .ok_or("pick <x> <y>")?;
            let y = opts
                .get(1)
                .and_then(|v| v.parse::<i64>().ok())
                .ok_or("pick <x> <y>")?;
            let v = call(&mut c, "pick", J::obj().set("x", x).set("y", y))?;
            for n in v.get("stack").map(J::arr).unwrap_or(&[]) {
                println!(
                    "#{} {:<7} {:?} {}",
                    n.get("index").and_then(J::as_i64).unwrap_or(0),
                    n.get("kind").and_then(J::as_str).unwrap_or(""),
                    n.get("text").and_then(J::as_str),
                    if n.get("handler").and_then(J::as_bool).unwrap_or(false) {
                        "handler"
                    } else {
                        ""
                    }
                );
            }
        }
        "deps" => {
            let mut p = J::obj();
            if let Some(i) = opts.iter().position(|o| o == "--word") {
                p.put(
                    "word",
                    opts.get(i + 1)
                        .and_then(|v| v.parse::<i64>().ok())
                        .ok_or("--word n")?,
                );
            } else if let Some(i) = opts.iter().position(|o| o == "--field") {
                p.put("field", opts.get(i + 1).ok_or("--field name")?.as_str());
            } else {
                return Err("deps --word n | --field name".into());
            }
            println!("{}", json::to_string(&call(&mut c, "deps", p)?));
        }
        "why" => println!("{}", json::to_string(&call(&mut c, "why", J::obj())?)),
        "counters" => println!("{}", json::to_string(&call(&mut c, "counters", J::obj())?)),
        "state" => {
            let v = call(&mut c, "state", J::obj())?;
            for f in v.get("fields").map(J::arr).unwrap_or(&[]) {
                println!(
                    "{:<12} {:<6} {:<20} {}{}",
                    f.get("name").and_then(J::as_str).unwrap_or(""),
                    f.get("kind").and_then(J::as_str).unwrap_or(""),
                    value_text(f.get("value").unwrap_or(&J::Null)),
                    f.get("writable")
                        .and_then(J::as_bool)
                        .unwrap_or(false)
                        .then_some("writable")
                        .unwrap_or("read-only"),
                    f.get("word")
                        .and_then(J::as_i64)
                        .map(|w| format!("  word {w}"))
                        .unwrap_or_default(),
                );
            }
        }
        "set" => {
            let field = opts.first().ok_or("set <field> <kind> <value>")?;
            let kind = opts.get(1).ok_or("set <field> <kind> <value>")?;
            let value = opts.get(2).ok_or("set <field> <kind> <value>")?;
            let v = call(
                &mut c,
                "set",
                J::obj()
                    .set("field", field.as_str())
                    .set("value", value_of(kind, value)?),
            )?;
            println!("{}", json::to_string(&v));
        }
        "click" | "pointer" | "down" | "up" => {
            let x = opts
                .first()
                .and_then(|v| v.parse::<i64>().ok())
                .ok_or("x y")?;
            let y = opts
                .get(1)
                .and_then(|v| v.parse::<i64>().ok())
                .ok_or("x y")?;
            let v = call(&mut c, cmd, J::obj().set("x", x).set("y", y))?;
            println!("{}", json::to_string(&v));
        }
        "key" => {
            let key = opts.first().ok_or("key <name>")?;
            let v = call(&mut c, "key", J::obj().set("key", key.as_str()))?;
            println!("{}", json::to_string(&v));
        }
        "type" => {
            let text = opts.first().ok_or("type <text>")?;
            let v = call(&mut c, "text", J::obj().set("text", text.as_str()))?;
            println!("{}", json::to_string(&v));
        }
        "tick" => {
            let ms = opts
                .first()
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(16);
            println!(
                "{}",
                json::to_string(&call(&mut c, "tick", J::obj().set("ms", ms))?)
            );
        }
        "resize" => {
            let w = opts
                .first()
                .and_then(|v| v.parse::<i64>().ok())
                .ok_or("resize <w> <h> [scale]")?;
            let h = opts
                .get(1)
                .and_then(|v| v.parse::<i64>().ok())
                .ok_or("resize <w> <h> [scale]")?;
            let scale = opts
                .get(2)
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(1.0);
            println!(
                "{}",
                json::to_string(&call(
                    &mut c,
                    "resize",
                    J::obj().set("w", w).set("h", h).set("scale", scale)
                )?)
            );
        }
        "reload" => println!("{}", json::to_string(&call(&mut c, "reload", J::obj())?)),
        "shot" => {
            let path = opts.first().ok_or("shot <file.png>")?;
            let v = call(&mut c, "shot", J::obj())?;
            let b64 = v.get("png").and_then(J::as_str).ok_or("no image")?;
            let bytes = json::base64_decode(b64).map_err(|e| e.to_string())?;
            // a PNG from the wire is checked before it is written
            png::decode(&bytes).map_err(|e| e.to_string())?;
            std::fs::write(path, &bytes).map_err(|e| format!("{path}: {e}"))?;
            println!(
                "{} {}x{} {} bytes digest {}",
                path,
                v.get("w").and_then(J::as_i64).unwrap_or(0),
                v.get("h").and_then(J::as_i64).unwrap_or(0),
                bytes.len(),
                v.get("digest").and_then(J::as_str).unwrap_or("")
            );
        }
        "diff" => {
            let path = opts.first().ok_or("diff <golden.png>")?;
            let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
            let v = call(
                &mut c,
                "diff",
                J::obj().set("golden", json::base64_encode(&bytes)),
            )?;
            println!("{}", json::to_string(&v));
            if v.get("match").and_then(J::as_bool) != Some(true) {
                return Err("the frame differs from the golden".into());
            }
        }
        "stop" | "shutdown" => {
            let v = call(&mut c, "shutdown", J::obj())?;
            println!("{}", json::to_string(&v));
        }
        other => return Err(format!("no inspect command {other:?}")),
    }
    Ok(())
}
