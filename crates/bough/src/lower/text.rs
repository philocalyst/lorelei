//! Text for the lowered runtime (Integration Card 3, decision D4): one store of shaped runs, a full shaper with
//! face fallback behind it, a coverage rasteriser, colour bitmaps where a face has them.
//!
//! A run is a line fragment shaped once: glyphs with their pens in device pixels, memoised by (text, size). A
//! record names a run by index and generation, so a record that outlives its run draws nothing instead of
//! another text. Text no face in the chain covers is shaped with the first face and shows that face's
//! missing-glyph box: it is never an empty run.
//!
//! The admitted contract, stated: shaping is per fallback segment, left to right or right to left as the
//! segment's script says; reordering of mixed-direction text across segments (UAX 9) is not done. Lines break
//! at spaces and after CJK characters, greedily. Glyphs sit on whole device pixels.
use std::collections::HashMap;

/// Font bytes kept for the process: a face borrows them. One copy per distinct file content.
fn intern(bytes: &[u8]) -> &'static [u8] {
    use std::sync::{Mutex, OnceLock};
    static FONTS: OnceLock<Mutex<HashMap<(usize, u64), &'static [u8]>>> = OnceLock::new();
    let h = bytes
        .iter()
        .step_by(97)
        .fold(0xcbf2_9ce4_8422_2325u64, |h, &b| {
            (h ^ b as u64).wrapping_mul(0x100_0000_01b3)
        });
    let mut g = FONTS.get_or_init(Default::default).lock().unwrap();
    if let Some(&k) = g.get(&(bytes.len(), h)) {
        if k == bytes {
            return k;
        }
    }
    let k: &'static [u8] = Box::leak(bytes.to_vec().into_boxed_slice());
    g.insert((bytes.len(), h), k);
    k
}

/// A font file mapped once for the process (a fallback face can be hundreds of megabytes: it is mapped, not
/// read, and only the pages a glyph needs are ever touched). `None` if the file cannot be opened or mapped.
fn font_file(path: &str) -> Option<&'static [u8]> {
    use std::os::fd::AsRawFd;
    use std::sync::{Mutex, OnceLock};
    static FILES: OnceLock<Mutex<HashMap<String, Option<&'static [u8]>>>> = OnceLock::new();
    let mut g = FILES.get_or_init(Default::default).lock().unwrap();
    if let Some(&f) = g.get(path) {
        return f;
    }
    let map = (|| {
        let file = std::fs::File::open(path).ok()?;
        let len = file.metadata().ok()?.len() as usize;
        if len == 0 {
            return None;
        }
        // SAFETY: a private read-only mapping of `len` bytes of an open file; the kernel picks the address.
        // The result is checked against MAP_FAILED before use. The mapping is never unmapped, so the slice
        // is valid for the process's life; it is private, so writes by others to the file are not seen as
        // changes of mapped pages already copied, and a truncation of a system font file while the process
        // runs is outside what this guards (the same holds for any mapped font).
        let p = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ,
                libc::MAP_PRIVATE,
                file.as_raw_fd(),
                0,
            )
        };
        if p == libc::MAP_FAILED {
            return None;
        }
        // SAFETY: `p` is a live mapping of `len` readable bytes that is never unmapped.
        Some(unsafe { std::slice::from_raw_parts(p as *const u8, len) })
    })();
    g.insert(path.to_string(), map);
    map
}

struct FaceD {
    rb: rustybuzz::Face<'static>,
    upem: f32,
}

/// One glyph of a run: which face, which glyph, where its origin is relative to the run's pen origin (device
/// pixels, y down from the baseline).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct G {
    pub face: u8,
    pub gid: u16,
    pub x: i32,
    pub y: i32,
}

pub struct Run {
    pub glyphs: Vec<G>,
    /// Advance width in device pixels.
    pub w: f32,
    /// Ink bounds relative to the pen origin on the baseline: x0, y0, x1, y1.
    pub ink: [i32; 4],
    pub size: f32,
    generation: u32,
    stamp: u32,
    key: (Box<str>, u32),
    live: bool,
}

/// A glyph's pixels: coverage (one byte per pixel) or premultiplied RGBA (a colour bitmap).
pub struct Bitmap {
    pub w: i32,
    pub h: i32,
    /// The bitmap's top-left relative to the glyph origin on the baseline (y down).
    pub left: i32,
    pub top: i32,
    pub color: bool,
    pub data: Vec<u8>,
}

/// Counters of what the text system did (count bands for the gates).
#[derive(Default, Clone, Copy, Debug)]
pub struct TextStats {
    pub shaped: u64,
    pub memo_hits: u64,
    pub rasterised: u64,
    pub fallback_segments: u64,
    pub evicted: u64,
}

pub struct TextSys {
    faces: Vec<FaceD>,
    /// Fallback files not yet opened (opened the first time the faces before them miss a character).
    pending: Vec<String>,
    runs: Vec<Run>,
    free: Vec<u32>,
    /// Runs by size (its bits), then by text: a lookup borrows the text and allocates nothing.
    memo: HashMap<u32, HashMap<Box<str>, u32>>,
    glyphs: HashMap<(u8, u16, u32), Bitmap>,
    pass: u32,
    pub stats: TextStats,
}

/// Where the system keeps faces that cover what a UI face does not (colour emoji, CJK, symbols). A file that is
/// not there is skipped.
fn system_fallbacks() -> Vec<String> {
    let mut v: Vec<String> = std::env::var("BOUGH_FALLBACK_FONTS")
        .map(|s| s.split(':').map(String::from).collect())
        .unwrap_or_default();
    #[cfg(target_os = "macos")]
    v.extend(
        [
            "/System/Library/Fonts/Apple Color Emoji.ttc",
            "/System/Library/Fonts/PingFang.ttc",
            "/System/Library/Fonts/Hiragino Sans GB.ttc",
            "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
            "/System/Library/Fonts/SFArabic.ttf",
            "/System/Library/Fonts/Apple Symbols.ttf",
        ]
        .map(String::from),
    );
    #[cfg(not(target_os = "macos"))]
    v.extend(
        [
            "/usr/share/fonts/truetype/noto/NotoColorEmoji.ttf",
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        ]
        .map(String::from),
    );
    v
}

impl TextSys {
    pub fn new(font: &[u8]) -> TextSys {
        let mut t = TextSys {
            faces: Vec::new(),
            pending: system_fallbacks(),
            runs: Vec::new(),
            free: Vec::new(),
            memo: HashMap::new(),
            glyphs: HashMap::new(),
            pass: 0,
            stats: TextStats::default(),
        };
        assert!(
            t.add_face(font),
            "the application font is not a font rustybuzz can open"
        );
        t.pending.reverse();
        t
    }
    fn add_face(&mut self, bytes: &[u8]) -> bool {
        self.add_static(intern(bytes))
    }
    fn add_static(&mut self, data: &'static [u8]) -> bool {
        match rustybuzz::Face::from_slice(data, 0) {
            Some(rb) if self.faces.len() < 255 => {
                let upem = rb.units_per_em() as f32;
                self.faces.push(FaceD { rb, upem });
                true
            }
            _ => false,
        }
    }
    /// The first face that has a glyph for `c`, opening fallback files as needed; face 0 if none has.
    fn face_for(&mut self, c: char) -> u8 {
        loop {
            if let Some(i) = self
                .faces
                .iter()
                .position(|f| f.rb.glyph_index(c).is_some())
            {
                return i as u8;
            }
            let Some(path) = self.pending.pop() else {
                return 0;
            };
            if let Some(bytes) = font_file(&path) {
                self.add_static(bytes);
            }
        }
    }
    /// Ascent and descent (positive down) of the primary face at `size` device pixels.
    pub fn metrics(&self, size: f32) -> (f32, f32) {
        let f = &self.faces[0];
        (
            f.rb.ascender() as f32 * size / f.upem,
            -(f.rb.descender() as f32) * size / f.upem,
        )
    }
    /// Where the baseline is in a line box of height `lh`: the face's ascent and descent centred in it.
    pub fn baseline(&self, size: f32, lh: f32) -> i32 {
        let (a, d) = self.metrics(size);
        ((lh - (a + d)) / 2.0 + a).round() as i32
    }
    /// A new description pass begins: runs are stamped as they are asked for.
    pub fn begin_pass(&mut self) {
        self.pass = self.pass.wrapping_add(1);
        if self.pass % 256 == 0 {
            // runs nobody asked for in the last 256 descriptions are released; their slots get a new generation
            for (i, r) in self.runs.iter_mut().enumerate() {
                if r.live && self.pass.wrapping_sub(r.stamp) > 256 {
                    if let Some(m) = self.memo.get_mut(&r.key.1) {
                        m.remove(&r.key.0);
                    }
                    r.live = false;
                    r.glyphs = Vec::new();
                    r.generation = r.generation.wrapping_add(1);
                    self.free.push(i as u32);
                    self.stats.evicted += 1;
                }
            }
        }
    }
    /// The run of `text` at `size` device pixels: (index, generation). Shaped the first time it is asked for.
    pub fn run(&mut self, text: &str, size: f32) -> (u32, u32) {
        let bits = size.to_bits();
        if let Some(&i) = self.memo.get(&bits).and_then(|m| m.get(text)) {
            let r = &mut self.runs[i as usize];
            r.stamp = self.pass;
            self.stats.memo_hits += 1;
            return (i, r.generation);
        }
        let (glyphs, w) = self.shape(text, size);
        let mut ink = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
        for g in glyphs.clone() {
            let b = self.bitmap(g.face, g.gid, size);
            if b.w > 0 && b.h > 0 {
                ink[0] = ink[0].min(g.x + b.left);
                ink[1] = ink[1].min(g.y + b.top);
                ink[2] = ink[2].max(g.x + b.left + b.w);
                ink[3] = ink[3].max(g.y + b.top + b.h);
            }
        }
        if ink[0] > ink[2] {
            ink = [0; 4];
        }
        let key = (Box::<str>::from(text), bits);
        let run = Run {
            glyphs,
            w,
            ink,
            size,
            generation: 1,
            stamp: self.pass,
            key: key.clone(),
            live: true,
        };
        let i = match self.free.pop() {
            Some(i) => {
                let generation = self.runs[i as usize].generation;
                self.runs[i as usize] = Run { generation, ..run };
                i
            }
            None => {
                self.runs.push(run);
                self.runs.len() as u32 - 1
            }
        };
        self.memo.entry(bits).or_default().insert(key.0, i);
        (i, self.runs[i as usize].generation)
    }
    /// The run a record names, if it is still that run.
    pub fn get(&self, i: u32, generation: u32) -> Option<&Run> {
        self.runs
            .get(i as usize)
            .filter(|r| r.live && r.generation == generation)
    }
    pub fn live_runs(&self) -> usize {
        self.runs.iter().filter(|r| r.live).count()
    }
    fn shape(&mut self, text: &str, size: f32) -> (Vec<G>, f32) {
        self.stats.shaped += 1;
        // fallback segments: maximal stretches of characters whose first covering face is the same
        let mut segs: Vec<(u8, usize, usize)> = Vec::new();
        let mut rtl = false;
        for (at, c) in text.char_indices() {
            // joiners, selectors and marks stay with the character before them; a space stays inside a
            // right-to-left stretch (so its words keep their order) and is otherwise the UI face's
            let glue = matches!(c, '\u{200d}' | '\u{fe00}'..='\u{fe0f}' | '\u{300}'..='\u{36f}' | '\u{1f3fb}'..='\u{1f3ff}')
                || (c == ' ' && rtl);
            if c != ' ' {
                rtl = matches!(c as u32, 0x590..=0x8ff | 0xfb1d..=0xfdff | 0xfe70..=0xfeff);
            }
            let face = match segs.last() {
                Some(&(f, ..))
                    if glue
                        || self.faces[f as usize].rb.glyph_index(c).is_some()
                            && (f == 0 || self.faces[0].rb.glyph_index(c).is_none()) =>
                {
                    f
                }
                _ => self.face_for(c),
            };
            match segs.last_mut() {
                Some(s) if s.0 == face => s.2 = at + c.len_utf8(),
                _ => segs.push((face, at, at + c.len_utf8())),
            }
        }
        if segs.len() > 1 {
            self.stats.fallback_segments += segs.len() as u64 - 1;
        }
        let mut out = Vec::new();
        let mut pen = 0f32;
        for (face, a, b) in segs {
            let f = &self.faces[face as usize];
            let k = size / f.upem;
            let mut buf = rustybuzz::UnicodeBuffer::new();
            buf.push_str(&text[a..b]);
            buf.guess_segment_properties();
            let shaped = rustybuzz::shape(&f.rb, &[], buf);
            for (info, pos) in shaped.glyph_infos().iter().zip(shaped.glyph_positions()) {
                out.push(G {
                    face,
                    gid: info.glyph_id as u16,
                    x: (pen + pos.x_offset as f32 * k).round() as i32,
                    y: (-(pos.y_offset as f32) * k).round() as i32,
                });
                pen += pos.x_advance as f32 * k;
            }
        }
        (out, pen)
    }
    /// The pixels of a glyph at a size, rasterised the first time.
    pub fn bitmap(&mut self, face: u8, gid: u16, size: f32) -> &Bitmap {
        let key = (face, gid, size.to_bits());
        if !self.glyphs.contains_key(&key) {
            self.stats.rasterised += 1;
            let b = raster(&self.faces[face as usize], gid, size);
            self.glyphs.insert(key, b);
        }
        &self.glyphs[&key]
    }
    pub fn bitmap_ready(&self, face: u8, gid: u16, size: f32) -> Option<&Bitmap> {
        self.glyphs.get(&(face, gid, size.to_bits()))
    }
    /// Words of a paragraph: each a run and its width, with the break after it (a space of `space` pixels, or
    /// none: a CJK character, or a hard newline).
    pub fn words(&mut self, text: &str, size: f32) -> Para {
        let space = self.run(" ", size);
        let space_w = self.runs[space.0 as usize].w;
        let mut words = Vec::new();
        let mut push = |t: &mut TextSys, s: &str, brk: Brk| {
            let (run, generation) = if s.is_empty() {
                (u32::MAX, 0)
            } else {
                t.run(s, size)
            };
            let w = if s.is_empty() {
                0.0
            } else {
                t.runs[run as usize].w
            };
            words.push(Word {
                run,
                generation,
                w,
                brk,
            });
        };
        let mut start = 0;
        let mut it = text.char_indices().peekable();
        while let Some((at, c)) = it.next() {
            let end = at + c.len_utf8();
            if c == ' ' || c == '\n' {
                push(
                    self,
                    &text[start..at],
                    if c == ' ' { Brk::Space } else { Brk::Line },
                );
                start = end;
            } else if cjk(c) && it.peek().is_some_and(|&(_, n)| n != ' ' && n != '\n') {
                push(self, &text[start..end], Brk::Soft);
                start = end;
            }
        }
        push(self, &text[start..], Brk::Line);
        Para {
            words,
            space: space_w,
            lh: 0.0,
        }
    }
}

fn cjk(c: char) -> bool {
    matches!(c as u32, 0x2e80..=0x9fff | 0xac00..=0xd7af | 0xf900..=0xfaff | 0xff00..=0xffef | 0x20000..=0x3ffff)
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Brk {
    /// A space: a break opportunity that is `space` wide when not taken.
    Space,
    /// A break opportunity of no width.
    Soft,
    /// A forced break.
    Line,
}
#[derive(Clone, Copy, Debug)]
pub struct Word {
    pub run: u32,
    pub generation: u32,
    pub w: f32,
    pub brk: Brk,
}
/// A paragraph as layout sees it: words to fill lines with.
#[derive(Clone, Debug)]
pub struct Para {
    pub words: Vec<Word>,
    pub space: f32,
    /// Line height in device pixels.
    pub lh: f32,
}
impl Para {
    /// Greedy lines at `width`: `f(line, word index, x)` for every word; returns (lines, widest line, the
    /// smallest width that would give these lines, the width at which a line would take one more word).
    pub fn lines(
        &self,
        width: f32,
        mut f: impl FnMut(usize, usize, f32),
    ) -> (usize, f32, f32, f32) {
        let (mut line, mut x, mut widest, mut grow) = (0usize, 0f32, 0f32, f32::INFINITY);
        let mut fresh = true;
        let mut gap = 0f32;
        for (i, w) in self.words.iter().enumerate() {
            if !fresh && x + gap + w.w > width {
                grow = grow.min(x + gap + w.w);
                widest = widest.max(x);
                line += 1;
                x = 0.0;
                gap = 0.0;
            }
            let at = x + gap;
            f(line, i, at);
            x = at + w.w;
            fresh = false;
            match w.brk {
                Brk::Space => gap = self.space,
                Brk::Soft => gap = 0.0,
                Brk::Line => {
                    widest = widest.max(x);
                    if i + 1 < self.words.len() {
                        line += 1;
                        x = 0.0;
                        gap = 0.0;
                        fresh = true;
                    }
                }
            }
        }
        (line + 1, widest, widest, grow)
    }
}
impl bough_lay::Wrap for Para {
    fn inline(&self) -> (f32, f32) {
        let min = self.words.iter().fold(0f32, |m, w| m.max(w.w));
        let (_, max, ..) = self.lines(f32::INFINITY, |_, _, _| {});
        (min.ceil(), max.ceil())
    }
    fn block(&self, w: f32) -> bough_lay::Block {
        let (n, widest, lo, hi) = self.lines(w, |_, _, _| {});
        bough_lay::Block {
            w: widest.ceil(),
            h: n as f32 * self.lh,
            lo,
            hi,
        }
    }
    fn key(&self) -> [u32; 2] {
        [
            self.words.len() as u32,
            self.words.first().map_or(0, |w| w.run),
        ]
    }
    fn rc(&self) -> std::rc::Rc<dyn bough_lay::Wrap> {
        std::rc::Rc::new(self.clone())
    }
}

// ---- rasterising ---------------------------------------------------------------------------------------------

/// Signed-area accumulation (the method of font-rs): each edge adds its covered area to the pixels it crosses
/// and its full height to the pixel after; a running sum along a row is the coverage.
struct Acc {
    w: usize,
    h: usize,
    a: Vec<f32>,
    at: (f32, f32),
    start: (f32, f32),
    k: f32,
    ox: f32,
    oy: f32,
}
impl Acc {
    fn line(&mut self, p0: (f32, f32), p1: (f32, f32)) {
        if p0.1 == p1.1 {
            return;
        }
        let (dir, p0, p1) = if p0.1 < p1.1 {
            (1.0, p0, p1)
        } else {
            (-1.0, p1, p0)
        };
        let dxdy = (p1.0 - p0.0) / (p1.1 - p0.1);
        let mut x = p0.0;
        let y0 = p0.1.max(0.0) as usize;
        if p0.1 < 0.0 {
            x -= p0.1 * dxdy;
        }
        for y in y0..self.h.min(p1.1.ceil().max(0.0) as usize) {
            let row = y * self.w;
            let dy = ((y + 1) as f32).min(p1.1) - (y as f32).max(p0.1);
            let xnext = x + dxdy * dy;
            let d = dy * dir;
            let (x0, x1) = if x < xnext { (x, xnext) } else { (xnext, x) };
            let x0floor = x0.floor();
            let x0i = (x0floor as i32).clamp(0, self.w as i32 - 2);
            let x1ceil = x1.ceil();
            let x1i = (x1ceil as i32).clamp(0, self.w as i32 - 1);
            if x1i <= x0i + 1 {
                let xmf = 0.5 * (x + xnext) - x0floor;
                self.a[row + x0i as usize] += d - d * xmf;
                self.a[row + x0i as usize + 1] += d * xmf;
            } else {
                let s = (x1 - x0).recip();
                let x0f = x0 - x0floor;
                let a0 = 0.5 * s * (1.0 - x0f) * (1.0 - x0f);
                let x1f = x1 - x1ceil + 1.0;
                let am = 0.5 * s * x1f * x1f;
                self.a[row + x0i as usize] += d * a0;
                if x1i == x0i + 2 {
                    self.a[row + x0i as usize + 1] += d * (1.0 - a0 - am);
                } else {
                    let a1 = s * (1.5 - x0f);
                    self.a[row + x0i as usize + 1] += d * (a1 - a0);
                    for xi in x0i + 2..x1i - 1 {
                        self.a[row + xi as usize] += d * s;
                    }
                    let a2 = a1 + (x1i - x0i - 3) as f32 * s;
                    self.a[row + x1i as usize - 1] += d * (1.0 - a2 - am);
                }
                self.a[row + x1i as usize] += d * am;
            }
            x = xnext;
        }
    }
    fn map(&self, x: f32, y: f32) -> (f32, f32) {
        (x * self.k + self.ox, self.oy - y * self.k)
    }
}
impl ttf_parser::OutlineBuilder for Acc {
    fn move_to(&mut self, x: f32, y: f32) {
        self.at = self.map(x, y);
        self.start = self.at;
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.map(x, y);
        self.line(self.at, p);
        self.at = p;
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (p0, p1, p2) = (self.at, self.map(x1, y1), self.map(x, y));
        let dd = ((p0.0 - 2.0 * p1.0 + p2.0).powi(2) + (p0.1 - 2.0 * p1.1 + p2.1).powi(2)).sqrt();
        let n = ((dd * 0.75).sqrt().ceil() as usize).clamp(1, 32);
        let mut a = p0;
        for i in 1..=n {
            let t = i as f32 / n as f32;
            let u = 1.0 - t;
            let b = (
                u * u * p0.0 + 2.0 * u * t * p1.0 + t * t * p2.0,
                u * u * p0.1 + 2.0 * u * t * p1.1 + t * t * p2.1,
            );
            self.line(a, b);
            a = b;
        }
        self.at = p2;
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (p0, p1, p2, p3) = (self.at, self.map(x1, y1), self.map(x2, y2), self.map(x, y));
        let dd = ((p0.0 - 2.0 * p1.0 + p2.0).abs()
            + (p1.0 - 2.0 * p2.0 + p3.0).abs()
            + (p0.1 - 2.0 * p1.1 + p2.1).abs()
            + (p1.1 - 2.0 * p2.1 + p3.1).abs())
        .sqrt();
        let n = ((dd * 1.5).ceil() as usize).clamp(1, 48);
        let mut a = p0;
        for i in 1..=n {
            let t = i as f32 / n as f32;
            let u = 1.0 - t;
            let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
            let b = (
                w[0] * p0.0 + w[1] * p1.0 + w[2] * p2.0 + w[3] * p3.0,
                w[0] * p0.1 + w[1] * p1.1 + w[2] * p2.1 + w[3] * p3.1,
            );
            self.line(a, b);
            a = b;
        }
        self.at = p3;
    }
    fn close(&mut self) {
        let s = self.start;
        self.line(self.at, s);
        self.at = s;
    }
}

fn raster(f: &FaceD, gid: u16, size: f32) -> Bitmap {
    let id = ttf_parser::GlyphId(gid);
    let empty = Bitmap {
        w: 0,
        h: 0,
        left: 0,
        top: 0,
        color: false,
        data: Vec::new(),
    };
    // a colour bitmap strike (sbix, CBDT) first: an emoji face has no outlines worth drawing
    if let Some(img) = f.rb.glyph_raster_image(id, size.ceil() as u16) {
        if img.format == ttf_parser::RasterImageFormat::PNG {
            if let Some(b) = color_bitmap(&img, size) {
                return b;
            }
        }
    }
    let Some(bb) = f.rb.glyph_bounding_box(id) else {
        return empty;
    };
    let k = size / f.upem;
    let (x0, y0) = (
        (bb.x_min as f32 * k).floor() as i32 - 1,
        (-(bb.y_max as f32) * k).floor() as i32 - 1,
    );
    let (x1, y1) = (
        (bb.x_max as f32 * k).ceil() as i32 + 1,
        (-(bb.y_min as f32) * k).ceil() as i32 + 1,
    );
    let (w, h) = ((x1 - x0).max(0) as usize, (y1 - y0).max(0) as usize);
    if w == 0 || h == 0 || w > 4096 || h > 4096 {
        return empty;
    }
    let mut acc = Acc {
        w: w + 2,
        h,
        a: vec![0.0; (w + 2) * h + 2],
        at: (0.0, 0.0),
        start: (0.0, 0.0),
        k,
        ox: -(x0 as f32),
        oy: -(y0 as f32),
    };
    if f.rb.outline_glyph(id, &mut acc).is_none() {
        return empty;
    }
    let mut data = vec![0u8; w * h];
    for y in 0..h {
        let mut sum = 0f32;
        for x in 0..w {
            sum += acc.a[y * (w + 2) + x];
            data[y * w + x] = (sum.abs().min(1.0) * 255.0 + 0.5) as u8;
        }
    }
    Bitmap {
        w: w as i32,
        h: h as i32,
        left: x0,
        top: y0,
        color: false,
        data,
    }
}

/// A PNG strike scaled to `size`: premultiplied RGBA, box-filtered down (strikes are larger than text).
fn color_bitmap(img: &ttf_parser::RasterGlyphImage<'_>, size: f32) -> Option<Bitmap> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(img.data));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let (sw, sh) = (info.width as usize, info.height as usize);
    let px = |i: usize| -> [u32; 4] {
        match info.color_type {
            png::ColorType::Rgba => [
                buf[4 * i] as u32,
                buf[4 * i + 1] as u32,
                buf[4 * i + 2] as u32,
                buf[4 * i + 3] as u32,
            ],
            png::ColorType::Rgb => [
                buf[3 * i] as u32,
                buf[3 * i + 1] as u32,
                buf[3 * i + 2] as u32,
                255,
            ],
            png::ColorType::GrayscaleAlpha => [
                buf[2 * i] as u32,
                buf[2 * i] as u32,
                buf[2 * i] as u32,
                buf[2 * i + 1] as u32,
            ],
            _ => [buf[i] as u32, buf[i] as u32, buf[i] as u32, 255],
        }
    };
    let k = size / img.pixels_per_em.max(1) as f32;
    let (dw, dh) = (
        ((sw as f32 * k).round() as usize).max(1),
        ((sh as f32 * k).round() as usize).max(1),
    );
    let mut data = vec![0u8; dw * dh * 4];
    for y in 0..dh {
        let (ya, yb) = (
            y * sh / dh,
            ((y + 1) * sh / dh).max(y * sh / dh + 1).min(sh),
        );
        for x in 0..dw {
            let (xa, xb) = (
                x * sw / dw,
                ((x + 1) * sw / dw).max(x * sw / dw + 1).min(sw),
            );
            let mut acc = [0u32; 4];
            for sy in ya..yb {
                for sx in xa..xb {
                    let [r, g, b, a] = px(sy * sw + sx);
                    acc[0] += r * a;
                    acc[1] += g * a;
                    acc[2] += b * a;
                    acc[3] += a;
                }
            }
            let n = ((yb - ya) * (xb - xa)) as u32;
            let o = (y * dw + x) * 4;
            for c in 0..3 {
                data[o + c] = ((acc[c] + n * 127) / (n * 255)) as u8;
            }
            data[o + 3] = ((acc[3] + n / 2) / n) as u8;
        }
    }
    // the strike's origin: `x`, `y` are the bitmap's bottom-left from the glyph origin, in strike pixels, y up
    let left = (img.x as f32 * k).round() as i32;
    let top = -((img.y as f32 * k).round() as i32) - dh as i32;
    Some(Bitmap {
        w: dw as i32,
        h: dh as i32,
        left,
        top,
        color: true,
        data,
    })
}

/// A paragraph shared between the layout tree (which measures it) and the node (which emits its lines).
pub struct RcPara(pub std::rc::Rc<Para>);
impl bough_lay::Wrap for RcPara {
    fn inline(&self) -> (f32, f32) {
        self.0.inline()
    }
    fn block(&self, w: f32) -> bough_lay::Block {
        self.0.block(w)
    }
    fn key(&self) -> [u32; 2] {
        self.0.key()
    }
    fn rc(&self) -> std::rc::Rc<dyn bough_lay::Wrap> {
        self.0.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The mapping behind `font_file`: the bytes are the file's, and a second call is the same mapping.
    #[test]
    fn a_font_file_is_mapped_once_and_reads_as_the_file() {
        let path = std::env::temp_dir().join(format!("bough-font-map-{}.bin", std::process::id()));
        let bytes: Vec<u8> = (0..70_000u32).map(|i| (i * 31 % 251) as u8).collect();
        std::fs::write(&path, &bytes).unwrap();
        let a = font_file(path.to_str().unwrap()).expect("mapped");
        assert_eq!(a, &bytes[..]);
        let b = font_file(path.to_str().unwrap()).expect("mapped");
        assert_eq!(a.as_ptr(), b.as_ptr());
        assert!(font_file("/nonexistent/font.ttf").is_none());
        std::fs::remove_file(&path).unwrap();
    }
}
