//! JSON, hand-written for two reasons: the wire and the goldens are the same bytes (a schema change is a
//! visible diff), and the inspector adds no dependency an application that links it did not already have.
//!
//! Values keep object members in insertion order; [`write`] is canonical (fixed key order per builder, no
//! whitespace). The parser is a strict subset: no comments, no trailing commas, no duplicate-key rejection
//! beyond the last one winning (the writers here never emit duplicates).
use std::fmt::Write as _;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Arr(Vec<Value>),
    Obj(Vec<(String, Value)>),
}

impl Value {
    pub fn obj() -> Value {
        Value::Obj(Vec::new())
    }
    /// Builder: `Value::obj().set("a", 1)`.
    pub fn set(mut self, k: &str, v: impl Into<Value>) -> Value {
        if let Value::Obj(m) = &mut self {
            m.push((k.to_owned(), v.into()));
        }
        self
    }
    /// Add or replace a member (for parsed values).
    pub fn put(&mut self, k: &str, v: impl Into<Value>) {
        if let Value::Obj(m) = self {
            if let Some(slot) = m.iter_mut().find(|(n, _)| n == k) {
                slot.1 = v.into();
            } else {
                m.push((k.to_owned(), v.into()));
            }
        }
    }
    pub fn get(&self, k: &str) -> Option<&Value> {
        match self {
            Value::Obj(m) => m.iter().find(|(n, _)| n == k).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn idx(&self, i: usize) -> Option<&Value> {
        match self {
            Value::Arr(a) => a.get(i),
            _ => None,
        }
    }
    pub fn arr(&self) -> &[Value] {
        match self {
            Value::Arr(a) => a,
            _ => &[],
        }
    }
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            Value::Float(f) if f.fract() == 0.0 => Some(*f as i64),
            _ => None,
        }
    }
    pub fn as_u32(&self) -> Option<u32> {
        self.as_i64().and_then(|i| u32::try_from(i).ok())
    }
    pub fn as_usize(&self) -> Option<usize> {
        self.as_i64().and_then(|i| usize::try_from(i).ok())
    }
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(i) => Some(*i as f64),
            Value::Float(f) => Some(*f),
            _ => None,
        }
    }
    pub fn as_f32(&self) -> Option<f32> {
        self.as_f64().map(|f| f as f32)
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }
}
impl From<bool> for Value {
    fn from(b: bool) -> Value {
        Value::Bool(b)
    }
}
impl From<i64> for Value {
    fn from(i: i64) -> Value {
        Value::Int(i)
    }
}
impl From<u32> for Value {
    fn from(i: u32) -> Value {
        Value::Int(i as i64)
    }
}
impl From<u16> for Value {
    fn from(i: u16) -> Value {
        Value::Int(i as i64)
    }
}
impl From<u64> for Value {
    fn from(i: u64) -> Value {
        Value::Int(i as i64)
    }
}
impl From<usize> for Value {
    fn from(i: usize) -> Value {
        Value::Int(i as i64)
    }
}
impl From<i32> for Value {
    fn from(i: i32) -> Value {
        Value::Int(i as i64)
    }
}
impl From<f32> for Value {
    fn from(f: f32) -> Value {
        Value::Float(f as f64)
    }
}
impl From<f64> for Value {
    fn from(f: f64) -> Value {
        Value::Float(f)
    }
}
impl From<&str> for Value {
    fn from(s: &str) -> Value {
        Value::Str(s.to_owned())
    }
}
impl From<String> for Value {
    fn from(s: String) -> Value {
        Value::Str(s)
    }
}
impl From<Vec<Value>> for Value {
    fn from(v: Vec<Value>) -> Value {
        Value::Arr(v)
    }
}
impl<T: Into<Value>> From<Option<T>> for Value {
    fn from(o: Option<T>) -> Value {
        o.map_or(Value::Null, Into::into)
    }
}

/// Canonical JSON: no whitespace, object members in the order they were inserted, strings escaped, finite
/// floats in Rust's shortest round-trip form. Non-finite floats are written as strings (`"NaN"`, `"inf"`,
/// `"-inf"`) so a tool never produces invalid JSON.
pub fn write(v: &Value, out: &mut String) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Int(i) => {
            let _ = write!(out, "{i}");
        }
        Value::Float(f) => {
            if f.is_finite() {
                if f.fract() == 0.0 && f.abs() < 9.0e15 {
                    let _ = write!(out, "{}", *f as i64);
                } else {
                    let _ = write!(out, "{f}");
                }
            } else {
                let s = if f.is_nan() {
                    "NaN"
                } else if *f > 0.0 {
                    "inf"
                } else {
                    "-inf"
                };
                write_str(s, out);
            }
        }
        Value::Str(s) => write_str(s, out),
        Value::Arr(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i != 0 {
                    out.push(',');
                }
                write(x, out);
            }
            out.push(']');
        }
        Value::Obj(m) => {
            out.push('{');
            for (i, (k, x)) in m.iter().enumerate() {
                if i != 0 {
                    out.push(',');
                }
                write_str(k, out);
                out.push(':');
                write(x, out);
            }
            out.push('}');
        }
    }
}

pub fn to_string(v: &Value) -> String {
    let mut s = String::new();
    write(v, &mut s);
    s
}

fn write_str(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParseError {
    pub at: usize,
    pub what: &'static str,
}
impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "json: {} at byte {}", self.what, self.at)
    }
}
impl std::error::Error for ParseError {}

/// Parse one JSON value; trailing non-whitespace is an error.
pub fn parse(s: &str) -> Result<Value, ParseError> {
    let b = s.as_bytes();
    let mut p = Parser { b, i: 0 };
    p.ws();
    let v = p.value(0)?;
    p.ws();
    if p.i != b.len() {
        return Err(p.err("trailing bytes"));
    }
    Ok(v)
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}
impl Parser<'_> {
    fn err(&self, what: &'static str) -> ParseError {
        ParseError { at: self.i, what }
    }
    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }
    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.i += 1;
        }
    }
    fn eat(&mut self, c: u8) -> Result<(), ParseError> {
        if self.peek() == Some(c) {
            self.i += 1;
            Ok(())
        } else {
            Err(self.err("unexpected byte"))
        }
    }
    fn value(&mut self, depth: u32) -> Result<Value, ParseError> {
        if depth > 128 {
            return Err(self.err("nesting too deep"));
        }
        match self.peek().ok_or_else(|| self.err("unexpected end"))? {
            b'n' => {
                self.word("null")?;
                Ok(Value::Null)
            }
            b't' => {
                self.word("true")?;
                Ok(Value::Bool(true))
            }
            b'f' => {
                self.word("false")?;
                Ok(Value::Bool(false))
            }
            b'"' => Ok(Value::Str(self.string()?)),
            b'[' => {
                self.i += 1;
                let mut a = Vec::new();
                self.ws();
                if self.peek() == Some(b']') {
                    self.i += 1;
                    return Ok(Value::Arr(a));
                }
                loop {
                    self.ws();
                    a.push(self.value(depth + 1)?);
                    self.ws();
                    match self.peek() {
                        Some(b',') => self.i += 1,
                        Some(b']') => {
                            self.i += 1;
                            return Ok(Value::Arr(a));
                        }
                        _ => return Err(self.err("expected , or ]")),
                    }
                }
            }
            b'{' => {
                self.i += 1;
                let mut m = Vec::new();
                self.ws();
                if self.peek() == Some(b'}') {
                    self.i += 1;
                    return Ok(Value::Obj(m));
                }
                loop {
                    self.ws();
                    let k = self.string()?;
                    self.ws();
                    self.eat(b':')?;
                    self.ws();
                    let v = self.value(depth + 1)?;
                    if let Some(slot) = m.iter_mut().find(|(n, _): &&mut (String, Value)| *n == k) {
                        slot.1 = v;
                    } else {
                        m.push((k, v));
                    }
                    self.ws();
                    match self.peek() {
                        Some(b',') => self.i += 1,
                        Some(b'}') => {
                            self.i += 1;
                            return Ok(Value::Obj(m));
                        }
                        _ => return Err(self.err("expected , or }")),
                    }
                }
            }
            b'-' | b'0'..=b'9' => self.number(),
            _ => Err(self.err("not a value")),
        }
    }
    fn word(&mut self, w: &str) -> Result<(), ParseError> {
        if self.b[self.i..].starts_with(w.as_bytes()) {
            self.i += w.len();
            Ok(())
        } else {
            Err(self.err("bad literal"))
        }
    }
    fn number(&mut self) -> Result<Value, ParseError> {
        let start = self.i;
        if self.peek() == Some(b'-') {
            self.i += 1;
        }
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.i += 1;
        }
        let mut float = false;
        if self.peek() == Some(b'.') {
            float = true;
            self.i += 1;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.i += 1;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            float = true;
            self.i += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.i += 1;
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.i += 1;
            }
        }
        let s = core::str::from_utf8(&self.b[start..self.i]).map_err(|_| self.err("not utf-8"))?;
        if float {
            s.parse::<f64>()
                .map(Value::Float)
                .map_err(|_| self.err("bad number"))
        } else {
            s.parse::<i64>()
                .map(Value::Int)
                .map_err(|_| self.err("bad integer"))
        }
    }
    fn string(&mut self) -> Result<String, ParseError> {
        self.eat(b'"')?;
        let mut s = String::new();
        loop {
            let c = self.peek().ok_or_else(|| self.err("unterminated string"))?;
            self.i += 1;
            match c {
                b'"' => return Ok(s),
                b'\\' => {
                    let e = self.peek().ok_or_else(|| self.err("unterminated escape"))?;
                    self.i += 1;
                    match e {
                        b'"' => s.push('"'),
                        b'\\' => s.push('\\'),
                        b'/' => s.push('/'),
                        b'b' => s.push('\u{8}'),
                        b'f' => s.push('\u{c}'),
                        b'n' => s.push('\n'),
                        b'r' => s.push('\r'),
                        b't' => s.push('\t'),
                        b'u' => {
                            let mut u = 0u32;
                            for _ in 0..4 {
                                let d = self.peek().ok_or_else(|| self.err("short \\u"))?;
                                self.i += 1;
                                u = u * 16
                                    + match d {
                                        b'0'..=b'9' => (d - b'0') as u32,
                                        b'a'..=b'f' => (d - b'a' + 10) as u32,
                                        b'A'..=b'F' => (d - b'A' + 10) as u32,
                                        _ => return Err(self.err("bad hex")),
                                    };
                            }
                            if (0xD800..0xDC00).contains(&u) {
                                // a high surrogate: the low one must follow as \uXXXX
                                if !self.b[self.i..].starts_with(b"\\u") {
                                    return Err(self.err("lone surrogate"));
                                }
                                self.i += 2;
                                let mut l = 0u32;
                                for _ in 0..4 {
                                    let d = self.peek().ok_or_else(|| self.err("short \\u"))?;
                                    self.i += 1;
                                    l = l * 16
                                        + match d {
                                            b'0'..=b'9' => (d - b'0') as u32,
                                            b'a'..=b'f' => (d - b'a' + 10) as u32,
                                            b'A'..=b'F' => (d - b'A' + 10) as u32,
                                            _ => return Err(self.err("bad hex")),
                                        };
                                }
                                if !(0xDC00..0xE000).contains(&l) {
                                    return Err(self.err("bad low surrogate"));
                                }
                                u = 0x10000 + ((u - 0xD800) << 10) + (l - 0xDC00);
                            }
                            s.push(char::from_u32(u).ok_or_else(|| self.err("bad scalar"))?);
                        }
                        _ => return Err(self.err("bad escape")),
                    }
                }
                c if c < 0x80 => s.push(c as char),
                _ => {
                    // multi-byte UTF-8: copy the whole sequence
                    let start = self.i - 1;
                    let len = match c {
                        0xC0..=0xDF => 2,
                        0xE0..=0xEF => 3,
                        0xF0..=0xF7 => 4,
                        _ => return Err(self.err("bad utf-8 lead")),
                    };
                    while self.i < start + len {
                        self.i += 1;
                    }
                    let t = core::str::from_utf8(&self.b[start..self.i])
                        .map_err(|_| self.err("bad utf-8"))?;
                    s.push_str(t);
                }
            }
        }
    }
}

// ---- base64 (standard, padded): screenshots and goldens over a text transport -------------------------------

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64_encode(b: &[u8]) -> String {
    let mut s = String::with_capacity(b.len().div_ceil(3) * 4);
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        s.push(B64[(n >> 18) as usize & 63] as char);
        s.push(B64[(n >> 12) as usize & 63] as char);
        s.push(if c.len() > 1 {
            B64[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        s.push(if c.len() > 2 {
            B64[n as usize & 63] as char
        } else {
            '='
        });
    }
    s
}

pub fn base64_decode(s: &str) -> Result<Vec<u8>, ParseError> {
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let mut acc = 0u32;
    let mut n = 0u32;
    for (i, c) in s.bytes().enumerate() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => continue,
            b' ' | b'\n' | b'\r' | b'\t' => continue,
            _ => {
                return Err(ParseError {
                    at: i,
                    what: "bad base64",
                });
            }
        };
        acc = (acc << 6) | v as u32;
        n += 1;
        if n == 4 {
            out.push((acc >> 16) as u8);
            out.push((acc >> 8) as u8);
            out.push(acc as u8);
            acc = 0;
            n = 0;
        }
    }
    match n {
        0 => {}
        2 => out.push((acc >> 4) as u8),
        3 => {
            out.push((acc >> 10) as u8);
            out.push((acc >> 2) as u8);
        }
        _ => {
            return Err(ParseError {
                at: s.len(),
                what: "bad base64 length",
            });
        }
    }
    Ok(out)
}
