//! A blocking client for the inspector socket: what the terminal client, the dev server and the MCP server use.
use crate::json::{self, Value as J};
use crate::rpc::{RpcError, INVALID_REQUEST, PARSE_ERROR};
use crate::schema::SCHEMA_VERSION;
use crate::socket::{read_frame, write_frame};
use std::io;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub struct Client {
    stream: UnixStream,
    next: u64,
    handshaken: bool,
}
impl Client {
    /// Connect and say hello. Refuses a server that speaks another schema.
    pub fn open(path: impl AsRef<Path>) -> io::Result<Client> {
        let stream = UnixStream::connect(path)?;
        stream.set_read_timeout(Some(Duration::from_secs(30)))?;
        stream.set_write_timeout(Some(Duration::from_secs(30)))?;
        let mut c = Client {
            stream,
            next: 1,
            handshaken: false,
        };
        let hello = c.request_raw(
            "hello",
            &J::obj().set("schema", SCHEMA_VERSION as i64),
        )?;
        let v = hello.map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("hello refused: {} ({:?})", e.message, e.data),
            )
        })?;
        if v.get("schema").and_then(J::as_u32) != Some(SCHEMA_VERSION) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "the server speaks another schema version",
            ));
        }
        c.handshaken = true;
        Ok(c)
    }
    /// Send one request; the reply is a result or an RPC error.
    pub fn request(&mut self, method: &str, params: &J) -> io::Result<Result<J, RpcError>> {
        self.request_raw(method, params)
    }
    fn request_raw(&mut self, method: &str, params: &J) -> io::Result<Result<J, RpcError>> {
        let id = self.next;
        self.next += 1;
        let req = J::obj()
            .set("jsonrpc", "2.0")
            .set("id", id)
            .set("method", method)
            .set("params", params.clone());
        write_frame(&mut self.stream, json::to_string(&req).as_bytes())?;
        let bytes = read_frame(&mut self.stream)?;
        let v = json::parse(core::str::from_utf8(&bytes).unwrap_or("")).map_err(|e| {
            io::Error::new(io::ErrorKind::InvalidData, format!("reply: {e}"))
        })?;
        if let Some(e) = v.get("error") {
            let code = e.get("code").and_then(J::as_i64).unwrap_or(PARSE_ERROR as i64) as i32;
            let message = e
                .get("message")
                .and_then(J::as_str)
                .unwrap_or("no message")
                .to_owned();
            let data = e.get("data").cloned().unwrap_or(J::Null);
            return Ok(Err(RpcError {
                code,
                message,
                data,
            }));
        }
        Ok(Ok(v.get("result").cloned().unwrap_or(J::Null)))
    }
    pub fn handshaken(&self) -> bool {
        self.handshaken
    }
    /// A socket path short enough for a Unix address (the kernel admits about 104 bytes).
    pub fn short_path(dir: impl AsRef<Path>, tag: &str) -> PathBuf {
        let p = dir.as_ref().join(format!("bl-{}-{tag}.sock", std::process::id()));
        assert!(
            p.as_os_str().len() < 100,
            "socket path too long for a Unix address: {} ({} bytes)",
            p.display(),
            p.as_os_str().len()
        );
        p
    }
}

/// A refusable call: helper for the CLI and MCP (`Ok` or the RPC error's code and reason).
pub fn call(c: &mut Client, method: &str, params: &J) -> Result<J, (i32, String)> {
    match c.request(method, params) {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => Err((e.code, e.message)),
        Err(e) => Err((INVALID_REQUEST, e.to_string())),
    }
}
