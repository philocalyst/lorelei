//! `bough-mcp`: the MCP server. stdio by default; `--http ADDR` serves `POST /mcp` instead.
use bough_mcp::{Mcp, serve_http, serve_stream_owned};
use std::io::BufReader;
use std::net::TcpListener;
use std::path::PathBuf;

fn main() {
    let mut socket: Option<PathBuf> = None;
    let mut http: Option<String> = None;
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--socket" => socket = it.next().map(PathBuf::from),
            "--http" => http = it.next(),
            "--help" | "-h" => {
                println!("usage: bough-mcp [--socket PATH] [--http ADDR]");
                println!(
                    "  --socket PATH  the app's inspector socket (default: $BOUGH_INSPECT_SOCKET)"
                );
                println!("  --http ADDR    serve MCP at POST http://ADDR/mcp instead of stdio");
                std::process::exit(0);
            }
            other => {
                eprintln!("bough-mcp: unknown argument {other:?}");
                std::process::exit(2);
            }
        }
    }
    let socket = socket
        .or_else(bough_mcp::socket_from_env)
        .unwrap_or_else(|| {
            eprintln!("bough-mcp: no socket: pass --socket or set BOUGH_INSPECT_SOCKET");
            std::process::exit(2);
        });
    let mut mcp = Mcp::new(socket);
    match http {
        Some(addr) => {
            let listener = match TcpListener::bind(&addr) {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("bough-mcp: cannot bind {addr}: {e}");
                    std::process::exit(1);
                }
            };
            println!("listening {}", listener.local_addr().unwrap());
            if let Err(e) = serve_http(&mut mcp, listener) {
                eprintln!("bough-mcp: {e}");
                std::process::exit(1);
            }
        }
        None => {
            let stdin = std::io::stdin();
            let stdout = std::io::stdout();
            if let Err(e) = serve_stream_owned(&mut mcp, BufReader::new(stdin), stdout) {
                eprintln!("bough-mcp: {e}");
                std::process::exit(1);
            }
        }
    }
}
