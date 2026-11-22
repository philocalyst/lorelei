//! The agent/test socket: a Unix domain socket carrying length-framed JSON-RPC 2.0 (a 4-byte big-endian length
//! then the request bytes).
//!
//! The server is pumped from the application's own thread ([`Server::pump`]) or run blocking for a headless app
//! ([`Server::serve`]); a world's stores are raw pointers into its own allocations, so nothing here moves a
//! session across a thread. Connections share one serialized command stream. Each client must negotiate its
//! schema with `hello` before issuing commands.
use crate::json::{self, Value as J};
use crate::rpc::{self, INVALID_REQUEST, PARSE_ERROR, Request, RpcError};
use crate::schema::SCHEMA_VERSION;
use crate::session::{Program, Session};
use std::io::{self, Read, Write};
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

/// The largest request or response frame accepted (a screenshot PNG fits well inside).
pub const MAX_FRAME: usize = 64 * 1024 * 1024;

/// Write a complete length-prefixed frame to a blocking writer.
pub fn write_frame(w: &mut impl Write, bytes: &[u8]) -> io::Result<()> {
    if bytes.len() > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "frame too large",
        ));
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
fn answer<P: Program>(s: &mut Session<P>, req: &J, handshaken: &mut bool) -> (Response, bool) {
    let request = match Request::from_json(req) {
        Ok(r) => r,
        Err(e) => {
            let id = req.get("id").cloned().unwrap_or(J::Null);
            return (
                Response::Json(
                    J::obj()
                        .set("jsonrpc", "2.0")
                        .set("id", id)
                        .set("error", e.to_json()),
                ),
                false,
            );
        }
    };
    if !*handshaken && request.method != "hello" && request.method != "methods" {
        let e = RpcError::new(
            INVALID_REQUEST,
            "the first request on a connection must be hello",
        );
        return (Response::Json(request.error(&e)), false);
    }
    if request.method == "hello" {
        if (!*handshaken || request.params.get("schema").is_some())
            && request.params.get("schema").and_then(J::as_u32) != Some(SCHEMA_VERSION)
        {
            let e = RpcError::new(
                INVALID_REQUEST,
                format!("hello requires schema {SCHEMA_VERSION}"),
            );
            return (Response::Json(request.error(&e)), false);
        }
        *handshaken = true;
    }
    if request.method == "shot" {
        return match rpc::prepare_shot(s, &request.params) {
            Ok((frame, raw)) => (Response::Shot(request, frame, raw), false),
            Err(e) => (Response::Json(request.error(&e)), false),
        };
    }
    if request.method == "damage_map" {
        return match crate::server::damage_map(s, &request.params) {
            Ok(crate::server::DamageReply::Ready(v)) => (Response::Json(request.result(v)), false),
            Ok(crate::server::DamageReply::Heatmap { json, w, h, px }) => {
                (Response::Heatmap(request, json, w, h, px), false)
            }
            Err(e) => (Response::Json(request.error(&e)), false),
        };
    }
    let reply = rpc::handle(s, &request.method, &request.params);
    (
        Response::Json(match reply.result {
            Ok(v) => request.result(v),
            Err(e) => request.error(&e),
        }),
        reply.shutdown,
    )
}

/// Maximum number of simultaneous clients sharing a session.
const MAX_CLIENTS: usize = 8;
/// Per-client I/O budget on each pump. Large frames span multiple host ticks.
const IO_QUANTUM: usize = 64 * 1024;
const FRAME_TIMEOUT: Duration = Duration::from_secs(30);

enum Response {
    Json(J),
    Shot(Request, crate::Frame, bool),
    Heatmap(Request, J, i32, i32, Arc<[u32]>),
    #[cfg(test)]
    Thread(mpsc::SyncSender<std::thread::ThreadId>),
}

impl Response {
    fn bytes(self) -> io::Result<Vec<u8>> {
        let value = match self {
            Self::Json(value) => value,
            Self::Shot(request, frame, raw) => request.result(rpc::encode_shot(frame, raw)),
            Self::Heatmap(request, mut result, w, h, px) => {
                let encoded = crate::encoder::encode(w, h, &px, false);
                result.put("heatmap_png_base64", encoded.png_base64.as_str());
                request.result(result)
            }
            #[cfg(test)]
            Self::Thread(sender) => {
                let _ = sender.send(std::thread::current().id());
                J::Null
            }
        };
        let text = json::to_string(&value);
        let mut framed = Vec::new();
        write_frame(&mut framed, text.as_bytes())?;
        Ok(framed)
    }
}

type EncodeResult = io::Result<Vec<u8>>;
type EncodeJob = (Response, mpsc::SyncSender<EncodeResult>);

struct Encoders {
    tx: Option<mpsc::SyncSender<EncodeJob>>,
    threads: Vec<std::thread::JoinHandle<()>>,
}

impl Encoders {
    fn new() -> io::Result<Self> {
        let (send, recv) = mpsc::sync_channel::<EncodeJob>(MAX_CLIENTS);
        let recv = Arc::new(Mutex::new(recv));
        let mut threads = Vec::new();
        for _ in 0..2 {
            let recv = Arc::clone(&recv);
            threads.push(
                std::thread::Builder::new()
                    .name("bough-inspector-encode".into())
                    .spawn(move || {
                        loop {
                            let job = recv.lock().unwrap_or_else(|e| e.into_inner()).recv();
                            let Ok((reply, done)) = job else { break };
                            let _ = done.send(reply.bytes());
                        }
                    })?,
            );
        }
        Ok(Self {
            tx: Some(send),
            threads,
        })
    }

    fn submit(&self, reply: Response) -> Option<mpsc::Receiver<EncodeResult>> {
        let (send, recv) = mpsc::sync_channel(1);
        #[cfg(not(feature = "mutant-inline-reply"))]
        self.tx.as_ref()?.try_send((reply, send)).ok()?;
        #[cfg(feature = "mutant-inline-reply")]
        {
            let _ = &self.tx;
            send.send(reply.bytes()).ok()?;
        }
        Some(recv)
    }
}

impl Drop for Encoders {
    fn drop(&mut self) {
        // Drop the sender so the workers' recv() returns Err and they exit, then join them cleanly.
        self.tx = None;
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

/// A bound local socket. State lives on the application thread and commands execute serially.
pub struct Server {
    listener: UnixListener,
    path: PathBuf,
    identity: (u64, u64),
    connections: Vec<Conn>,
    encoders: Encoders,
    shut: bool,
}

struct Conn {
    stream: UnixStream,
    input: Vec<u8>,
    output: Vec<u8>,
    encoding: Option<mpsc::Receiver<EncodeResult>>,
    written: usize,
    handshaken: bool,
    shutdown_after_reply: bool,
    progress: Instant,
}

impl Conn {
    fn new(stream: UnixStream) -> Self {
        Self {
            stream,
            input: Vec::new(),
            output: Vec::new(),
            encoding: None,
            written: 0,
            handshaken: false,
            shutdown_after_reply: false,
            progress: Instant::now(),
        }
    }

    fn timed_out(&self) -> bool {
        (!self.handshaken
            || !self.input.is_empty()
            || !self.output.is_empty()
            || self.encoding.is_some())
            && self.progress.elapsed() >= FRAME_TIMEOUT
    }

    fn read_request(&mut self) -> io::Result<Option<Vec<u8>>> {
        let mut left = IO_QUANTUM;
        let mut chunk = [0; 16 * 1024];
        while left != 0 {
            let missing = if self.input.len() < 4 {
                4 - self.input.len()
            } else {
                let len = u32::from_be_bytes(self.input[..4].try_into().unwrap()) as usize;
                if len > MAX_FRAME {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "frame length above the limit",
                    ));
                }
                if self.input.len() == 4 + len {
                    return Ok(Some(std::mem::take(&mut self.input)));
                }
                4 + len - self.input.len()
            };
            let max_read = left.min(missing).min(chunk.len());
            match self.stream.read(&mut chunk[..max_read]) {
                Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
                Ok(n) => {
                    self.input.extend_from_slice(&chunk[..n]);
                    self.progress = Instant::now();
                    left -= n;
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(e),
            }
        }
        Ok(None)
    }

    /// Advance the reply without waiting for the client. Retain at most one response per peer.
    fn flush_reply(&mut self) -> io::Result<bool> {
        let end = self.output.len().min(self.written + IO_QUANTUM);
        if self.written < end {
            match self.stream.write(&self.output[self.written..end]) {
                Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                Ok(n) => {
                    self.written += n;
                    self.progress = Instant::now();
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(false),
                Err(e) => return Err(e),
            }
        }
        if self.written == self.output.len() {
            self.output = Vec::new();
            self.written = 0;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

impl Server {
    /// Bind a user-only socket. A socket file left by a process that died (the path exists, is a socket, is
    /// ours, and nothing is listening) is reclaimed; anything else at the path is left alone and bind fails.
    pub fn bind(path: impl AsRef<Path>) -> io::Result<Server> {
        let path = path.as_ref();
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let parent = parent.canonicalize()?;
        let name = path.file_name().ok_or(io::ErrorKind::InvalidInput)?;
        let path = parent.join(name);
        reclaim_stale(&path)?;
        let encoders = Encoders::new()?;
        let listener = UnixListener::bind(&path)?;
        let identity = socket_identity(&path)?;
        let server = Server {
            listener,
            path,
            identity,
            connections: Vec::new(),
            encoders,
            shut: false,
        };
        std::fs::set_permissions(&server.path, std::fs::Permissions::from_mode(0o600))?;
        server.listener.set_nonblocking(true)?;
        Ok(server)
    }

    /// Filesystem address of the bound socket.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Accept up to one backlog batch without blocking. Excess connections are closed.
    pub fn accept(&mut self) -> io::Result<bool> {
        let mut any = false;
        for _ in 0..MAX_CLIENTS {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    any = true;
                    if self.connections.len() < MAX_CLIENTS && same_user(&stream) {
                        stream.set_nonblocking(true)?;
                        self.connections.push(Conn::new(stream));
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(e),
            }
        }
        Ok(any)
    }

    /// Service up to one complete request per connection. I/O never waits on the peer.
    /// Command execution and painting stay on the caller thread. Workers encode owned pixels and replies.
    pub fn pump<P: Program>(&mut self, s: &mut Session<P>) -> io::Result<usize> {
        if self.shut {
            return Ok(0);
        }
        self.accept()?;
        let mut answered = 0;
        self.connections.retain_mut(|connection| {
            if self.shut || connection.timed_out() {
                self.shut |= connection.shutdown_after_reply;
                return false;
            }
            if let Some(encoding) = &connection.encoding {
                match encoding.try_recv() {
                    Ok(Ok(output)) => {
                        connection.encoding = None;
                        connection.output = output;
                        connection.progress = Instant::now();
                    }
                    Err(mpsc::TryRecvError::Empty) => return true,
                    _ => {
                        self.shut |= connection.shutdown_after_reply;
                        return false;
                    }
                }
            }
            if !connection.output.is_empty() {
                return match connection.flush_reply() {
                    Ok(false) => true,
                    Ok(true) | Err(_) if connection.shutdown_after_reply => {
                        self.shut = true;
                        false
                    }
                    Ok(true) => true,
                    Err(_) => false,
                };
            }
            let frame = match connection.read_request() {
                Ok(Some(frame)) => frame,
                Ok(None) => return true,
                Err(_) => return false,
            };
            let (out, shutdown) = match json::parse(core::str::from_utf8(&frame[4..]).unwrap_or(""))
            {
                Ok(value) => answer(s, &value, &mut connection.handshaken),
                Err(e) => (
                    Response::Json(J::obj().set("jsonrpc", "2.0").set("id", J::Null).set(
                        "error",
                        RpcError::new(PARSE_ERROR, format!("{e}")).to_json(),
                    )),
                    false,
                ),
            };
            answered += 1;
            connection.shutdown_after_reply = shutdown;
            connection.encoding = self.encoders.submit(out);
            connection.progress = Instant::now();
            if connection.encoding.is_none() {
                self.shut |= shutdown;
                return false;
            }
            true
        });
        Ok(answered)
    }

    /// Whether the shutdown command completed.
    pub fn shutdown(&self) -> bool {
        self.shut
    }

    /// Poll until shutdown. Closed and malformed connections leave the application running.
    pub fn serve<P: Program>(&mut self, s: &mut Session<P>) -> io::Result<()> {
        while !self.shut {
            let n = self.pump(s)?;
            if n == 0 {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        Ok(())
    }
}

/// Remove a socket file that no process is listening on, when it is this user's. A live socket is an
/// `AddrInUse` error; a socket of another user is a permission error; a regular file is left for `bind` to
/// refuse.
fn reclaim_stale(path: &Path) -> io::Result<()> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(_) => return Ok(()),
    };
    if !meta.file_type().is_socket() {
        return Ok(());
    }
    use std::os::unix::fs::MetadataExt;
    // SAFETY: geteuid takes no arguments and returns the current process's effective user ID.
    let mine = meta.uid() == unsafe { libc::geteuid() };
    match UnixStream::connect(path) {
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::AddrInUse,
            "another process is listening on this socket",
        )),
        Err(e)
            if mine
                && matches!(
                    e.kind(),
                    io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
                ) =>
        {
            // Another starter may already have replaced the stale inode.
            if socket_identity(path)? != (meta.dev(), meta.ino()) {
                return Err(io::ErrorKind::AddrInUse.into());
            }
            match std::fs::remove_file(path) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e),
            }
        }
        Err(e) if !mine => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("the socket belongs to another user: {e}"),
        )),
        Err(e) => Err(e),
    }
}

fn socket_identity(path: &Path) -> io::Result<(u64, u64)> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.file_type().is_socket() {
        return Err(io::ErrorKind::InvalidData.into());
    }
    Ok((metadata.dev(), metadata.ino()))
}

fn same_user(stream: &UnixStream) -> bool {
    // SAFETY: geteuid takes no arguments and returns the current process's effective user ID.
    stream
        .peer_cred()
        .is_ok_and(|peer| peer.uid == unsafe { libc::geteuid() })
}

impl Drop for Server {
    fn drop(&mut self) {
        if socket_identity(&self.path).is_ok_and(|identity| identity == self.identity) {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reply_serialization_runs_on_a_worker() {
        let encoders = Encoders::new().unwrap();
        let (sender, receiver) = mpsc::sync_channel(1);
        let reply = encoders.submit(Response::Thread(sender)).unwrap();
        let worker = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_ne!(
            worker,
            std::thread::current().id(),
            "reply serialized on the app thread"
        );
        assert!(reply.recv_timeout(Duration::from_secs(5)).unwrap().is_ok());
    }

    #[test]
    fn ten_thousand_random_and_truncated_frames_keep_bounded_read_buffers() {
        let mut seed = 19u32;
        let mut byte = || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 24) as u8
        };
        let (mut writer, reader) = UnixStream::pair().unwrap();
        reader.set_nonblocking(true).unwrap();
        let mut conn = Conn::new(reader);
        for i in 0..10_000 {
            let length = usize::from(byte());
            let data: Vec<u8> = (0..length).map(|_| byte()).collect();
            let mut frame = Vec::new();
            write_frame(&mut frame, &data).unwrap();
            let split = i % frame.len();
            writer.write_all(&frame[..split]).unwrap();
            assert!(conn.read_request().unwrap().is_none());
            assert!(conn.input.capacity() <= 1024);
            writer.write_all(&frame[split..]).unwrap();
            let decoded = conn.read_request().unwrap().unwrap();
            assert_eq!(&decoded[4..], &data);
            if let Ok(text) = std::str::from_utf8(&decoded[4..]) {
                let _ = json::parse(text);
            }
        }
        writer.write_all(&[0xde, 0, 0, 0]).unwrap();
        assert_eq!(
            conn.read_request().unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert!(conn.input.len() <= 4);
    }
}
