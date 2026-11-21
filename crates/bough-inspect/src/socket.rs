//! The agent/test socket: a Unix domain socket carrying length-framed JSON-RPC 2.0 (a 4-byte big-endian length
//! then the request bytes).
//!
//! The server is pumped from the application's own thread ([`Server::pump`]) or run blocking for a headless app
//! ([`Server::serve`]); a world's stores are raw pointers into its own allocations, so nothing here moves a
//! session across a thread. One connection may send many requests; the first request should be `hello`, and a
//! client that names a different schema version is refused before any command runs.
use crate::json::{self, Value as J};
use crate::rpc::{self, Request, RpcError, INVALID_REQUEST, PARSE_ERROR};
use crate::schema::SCHEMA_VERSION;
use crate::session::{Program, Session};
use std::io::{self, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

/// The largest request or response frame accepted (a screenshot PNG fits well inside).
pub const MAX_FRAME: usize = 64 * 1024 * 1024;

pub fn write_frame(w: &mut impl Write, bytes: &[u8]) -> io::Result<()> {
    if bytes.len() > MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "frame too large"));
    }
    w.write_all(&(bytes.len() as u32).to_be_bytes())?;
    w.write_all(bytes)?;
    w.flush()
}

/// Read one frame; `UnexpectedEof` at a clean end of stream.
pub fn read_frame(r: &mut impl Read) -> io::Result<Vec<u8>> {
    let mut n = [0u8; 4];
    r.read_exact(&mut n)?;
    let n = u32::from_be_bytes(n) as usize;
    if n > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame length above the limit",
        ));
    }
    let mut b = vec![0u8; n];
    r.read_exact(&mut b)?;
    Ok(b)
}

/// Answer one JSON-RPC message. `handshaken` is the connection's state.
fn answer<P: Program>(s: &mut Session<P>, req: &J, handshaken: &mut bool) -> (J, bool) {
    let request = match Request::from_json(req) {
        Ok(r) => r,
        Err(e) => {
            let id = req.get("id").cloned().unwrap_or(J::Null);
            return (
                J::obj()
                    .set("jsonrpc", "2.0")
                    .set("id", id)
                    .set("error", e.to_json()),
                false,
            );
        }
    };
    if !*handshaken {
        if request.method != "hello" && request.method != "methods" {
            let e = RpcError::new(
                INVALID_REQUEST,
                "the first request on a connection must be hello",
            );
            return (request.error(&e), false);
        }
        if request.method == "hello"
            && let Some(v) = request.params.get("schema").and_then(J::as_u32)
            && v != SCHEMA_VERSION
        {
            let e = RpcError::new(
                INVALID_REQUEST,
                format!("client speaks schema {v}, this build speaks {SCHEMA_VERSION}"),
            );
            return (request.error(&e), false);
        }
        *handshaken = true;
    }
    let reply = rpc::handle(s, &request.method, &request.params);
    (
        match reply.result {
            Ok(v) => request.result(v),
            Err(e) => request.error(&e),
        },
        reply.shutdown,
    )
}

/// A bound socket and at most one live connection.
pub struct Server {
    listener: UnixListener,
    path: PathBuf,
    conn: Option<Conn>,
    shut: bool,
}
struct Conn {
    stream: UnixStream,
    /// Bytes read but not yet a complete frame.
    buf: Vec<u8>,
    handshaken: bool,
}
impl Server {
    /// Bind a socket, removing a stale file at `path` first (a crashed dev server's).
    pub fn bind(path: impl AsRef<Path>) -> io::Result<Server> {
        let path = path.as_ref().to_path_buf();
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        let listener = UnixListener::bind(&path)?;
        listener.set_nonblocking(true)?;
        Ok(Server {
            listener,
            path,
            conn: None,
            shut: false,
        })
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    /// Accept one pending connection (non-blocking). A second connection replaces a dead one and is refused while
    /// one is live (one agent at a time: two would interleave commands on one world).
    pub fn accept(&mut self) -> io::Result<bool> {
        match self.listener.accept() {
            Ok((stream, _)) => {
                if self.conn.is_some() {
                    let mut s = stream;
                    let _ = write_frame(
                        &mut s,
                        json::to_string(
                            &J::obj()
                                .set("jsonrpc", "2.0")
                                .set("id", J::Null)
                                .set(
                                    "error",
                                    RpcError::new(INVALID_REQUEST, "a connection is already open")
                                        .to_json(),
                                ),
                        )
                        .as_bytes(),
                    );
                    return Ok(false);
                }
                stream.set_nonblocking(true)?;
                self.conn = Some(Conn {
                    stream,
                    buf: Vec::new(),
                    handshaken: false,
                });
                Ok(true)
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => Ok(false),
            Err(e) => Err(e),
        }
    }
    /// Service every complete request that has arrived, without blocking. Returns how many were answered.
    pub fn pump<P: Program>(&mut self, s: &mut Session<P>) -> io::Result<usize> {
        if self.conn.is_none() {
            self.accept()?;
        }
        if self.conn.is_none() {
            return Ok(0);
        }
        // read what is there
        let mut dead = false;
        {
            let c = self.conn.as_mut().expect("checked above");
            let mut chunk = [0u8; 16 * 1024];
            loop {
                match c.stream.read(&mut chunk) {
                    Ok(0) => {
                        dead = true;
                        break;
                    }
                    Ok(n) => c.buf.extend_from_slice(&chunk[..n]),
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                    Err(e) => return Err(e),
                }
            }
        }
        if dead {
            self.conn = None;
            return Ok(0);
        }
        let mut answered = 0;
        loop {
            let frame = {
                let c = self.conn.as_mut().expect("a connection");
                if c.buf.len() < 4 {
                    break;
                }
                let n = u32::from_be_bytes([c.buf[0], c.buf[1], c.buf[2], c.buf[3]]) as usize;
                if n > MAX_FRAME {
                    self.conn = None;
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "frame length above the limit",
                    ));
                }
                if c.buf.len() < 4 + n {
                    break;
                }
                let bytes: Vec<u8> = c.buf[4..4 + n].to_vec();
                c.buf.drain(..4 + n);
                bytes
            };
            let (out, shut) = {
                let c = self.conn.as_mut().expect("a connection");
                match json::parse(core::str::from_utf8(&frame).unwrap_or("")) {
                    Ok(v) => answer(s, &v, &mut c.handshaken),
                    Err(e) => (
                        J::obj()
                            .set("jsonrpc", "2.0")
                            .set("id", J::Null)
                            .set("error", RpcError::new(PARSE_ERROR, format!("{e}")).to_json()),
                        false,
                    ),
                }
            };
            let text = json::to_string(&out);
            let write = {
                let c = self.conn.as_mut().expect("a connection");
                let _ = c.stream.set_nonblocking(false);
                let r = write_frame(&mut c.stream, text.as_bytes());
                let _ = c.stream.set_nonblocking(true);
                r
            };
            if write.is_err() {
                self.conn = None;
            }
            answered += 1;
            if shut {
                self.shut = true;
                self.conn = None;
                break;
            }
            if self.conn.is_none() {
                break;
            }
        }
        Ok(answered)
    }
    pub fn shutdown(&self) -> bool {
        self.shut
    }
    /// Accept and serve, blocking, until a `shutdown` request or the client leaves.
    pub fn serve<P: Program>(&mut self, s: &mut Session<P>) -> io::Result<()> {
        loop {
            if self.conn.is_none() {
                match self.listener.accept() {
                    Ok((stream, _)) => {
                        stream.set_nonblocking(false)?;
                        self.conn = Some(Conn {
                            stream,
                            buf: Vec::new(),
                            handshaken: false,
                        });
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(2));
                        continue;
                    }
                    Err(e) => return Err(e),
                }
            }
            // a blocking read of exactly one frame, then dispatch
            let read = {
                let c = self.conn.as_mut().expect("a connection");
                read_frame(&mut c.stream)
            };
            let frame = match read {
                Ok(f) => f,
                Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                    self.conn = None;
                    continue;
                }
                Err(e) => return Err(e),
            };
            let (out, shut) = {
                let c = self.conn.as_mut().expect("a connection");
                match json::parse(core::str::from_utf8(&frame).unwrap_or("")) {
                    Ok(v) => answer(s, &v, &mut c.handshaken),
                    Err(e) => (
                        J::obj()
                            .set("jsonrpc", "2.0")
                            .set("id", J::Null)
                            .set("error", RpcError::new(PARSE_ERROR, format!("{e}")).to_json()),
                        false,
                    ),
                }
            };
            let write = {
                let c = self.conn.as_mut().expect("a connection");
                write_frame(&mut c.stream, json::to_string(&out).as_bytes())
            };
            if write.is_err() {
                self.conn = None;
            }
            if shut {
                self.shut = true;
                return Ok(());
            }
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
