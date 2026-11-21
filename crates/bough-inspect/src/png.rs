//! PNG for screenshots and goldens, with no dependency: 8-bit RGBA, filter 0, zlib "stored" deflate blocks.
//!
//! A stored-block stream is a legal zlib stream, just uncompressed: the pixels are not the expensive part of a
//! test screenshot and a byte-exact golden does not depend on a compressor's version. [`decode`] reads the same
//! subset back (any filter type, RGBA or RGB), which is what a golden diff needs.
//!
//! Words are `0xAARRGGBB`, as the core's colours and `Soft`'s framebuffer are.

pub const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

fn crc32(data: &[u8]) -> u32 {
    // table-less CRC-32 (IEEE); a screenshot is a few thousand blocks
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                (c >> 1) ^ 0xEDB8_8320
            } else {
                c >> 1
            };
        }
    }
    !c
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in data {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let at = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let c = crc32(&out[at..]);
    out.extend_from_slice(&c.to_be_bytes());
}

/// An RGBA PNG at the given size. `px` is row-major, `w * h` words, `0xAARRGGBB`.
pub fn encode(w: i32, h: i32, px: &[u32]) -> Vec<u8> {
    assert!(w >= 0 && h >= 0, "negative image size");
    assert_eq!(px.len(), (w as usize) * (h as usize), "pixel count");
    let mut raw = Vec::with_capacity((h as usize) * (1 + 4 * w as usize));
    for y in 0..h as usize {
        raw.push(0); // filter: none
        for x in 0..w as usize {
            let c = px[y * w as usize + x];
            raw.extend_from_slice(&[(c >> 16) as u8, (c >> 8) as u8, c as u8, (c >> 24) as u8]);
        }
    }
    let mut z = Vec::with_capacity(raw.len() + raw.len() / 65535 * 5 + 16);
    z.extend_from_slice(&[0x78, 0x01]); // deflate, 32 KiB window, no preset dictionary, fastest level
    let mut at = 0;
    while at < raw.len() {
        let n = (raw.len() - at).min(65535);
        let last = at + n >= raw.len();
        z.push(last as u8);
        z.extend_from_slice(&(n as u16).to_le_bytes());
        z.extend_from_slice(&(!(n as u16)).to_le_bytes());
        z.extend_from_slice(&raw[at..at + n]);
        at += n;
    }
    if raw.is_empty() {
        z.extend_from_slice(&[1, 0, 0, 0xff, 0xff]);
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());

    let mut out = Vec::with_capacity(z.len() + 64);
    out.extend_from_slice(&SIGNATURE);
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8 bits, RGBA, deflate, adaptive, no interlace
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

#[derive(Debug, PartialEq)]
pub enum PngError {
    NotPng,
    Truncated,
    Unsupported(&'static str),
    BadZlib,
    BadPixels,
}
impl core::fmt::Display for PngError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            PngError::NotPng => write!(f, "not a PNG"),
            PngError::Truncated => write!(f, "truncated PNG"),
            PngError::Unsupported(s) => write!(f, "unsupported PNG: {s}"),
            PngError::BadZlib => write!(f, "bad zlib stream"),
            PngError::BadPixels => write!(f, "bad pixel data"),
        }
    }
}
impl std::error::Error for PngError {}

/// A decoded 8-bit RGBA image as `0xAARRGGBB` words.
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    pub w: i32,
    pub h: i32,
    pub px: Vec<u32>,
}

pub fn decode(png: &[u8]) -> Result<Image, PngError> {
    if png.len() < 8 || png[..8] != SIGNATURE {
        return Err(PngError::NotPng);
    }
    let mut at = 8;
    let (mut w, mut h) = (0u32, 0u32);
    let (mut depth, mut color) = (0u8, 0u8);
    let mut idat = Vec::new();
    loop {
        if at + 8 > png.len() {
            return Err(PngError::Truncated);
        }
        let n = u32::from_be_bytes(png[at..at + 4].try_into().unwrap()) as usize;
        let kind = &png[at + 4..at + 8];
        if at + 12 + n > png.len() {
            return Err(PngError::Truncated);
        }
        let data = &png[at + 8..at + 8 + n];
        match kind {
            b"IHDR" => {
                if n != 13 {
                    return Err(PngError::BadPixels);
                }
                w = u32::from_be_bytes(data[0..4].try_into().unwrap());
                h = u32::from_be_bytes(data[4..8].try_into().unwrap());
                depth = data[8];
                color = data[9];
                if data[12] != 0 {
                    return Err(PngError::Unsupported("interlaced"));
                }
            }
            b"IDAT" => idat.extend_from_slice(data),
            b"IEND" => break,
            _ => {}
        }
        at += 12 + n;
    }
    if depth != 8 {
        return Err(PngError::Unsupported("bit depth"));
    }
    let bpp = match color {
        6 => 4, // RGBA
        2 => 3, // RGB
        _ => return Err(PngError::Unsupported("colour type")),
    };
    const MAX_PIXELS: u64 = 8_388_608;
    let pixels = u64::from(w) * u64::from(h);
    if pixels == 0 || pixels > MAX_PIXELS {
        return Err(PngError::Unsupported("image dimensions"));
    }
    let raw = inflate_stored(&idat)?;
    let stride = 1 + (w as usize) * bpp;
    if raw.len() != stride * h as usize {
        return Err(PngError::BadPixels);
    }
    let mut px = Vec::with_capacity(pixels as usize);
    let mut prev = vec![0u8; w as usize * bpp];
    let mut line = vec![0u8; w as usize * bpp];
    for y in 0..h as usize {
        let row = &raw[y * stride..(y + 1) * stride];
        let filter = row[0];
        let src = &row[1..];
        for i in 0..line.len() {
            let a = if i >= bpp { line[i - bpp] as i32 } else { 0 };
            let b = prev[i] as i32;
            let c = if i >= bpp { prev[i - bpp] as i32 } else { 0 };
            let x = src[i] as i32;
            line[i] = match filter {
                0 => x as u8,
                1 => (x + a) as u8,
                2 => (x + b) as u8,
                3 => (x + (a + b) / 2) as u8,
                4 => (x + paeth(a, b, c)) as u8,
                _ => return Err(PngError::Unsupported("filter")),
            };
        }
        for x in 0..w as usize {
            let p = &line[x * bpp..x * bpp + bpp];
            px.push(if bpp == 4 {
                (p[3] as u32) << 24 | (p[0] as u32) << 16 | (p[1] as u32) << 8 | p[2] as u32
            } else {
                0xff00_0000 | (p[0] as u32) << 16 | (p[1] as u32) << 8 | p[2] as u32
            });
        }
        core::mem::swap(&mut prev, &mut line);
    }
    Ok(Image {
        w: w as i32,
        h: h as i32,
        px,
    })
}

fn paeth(a: i32, b: i32, c: i32) -> i32 {
    let p = a + b - c;
    let (pa, pb, pc) = ((p - a).abs(), (p - b).abs(), (p - c).abs());
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// Inflate a zlib stream made only of stored blocks (what [`encode`] writes).
fn inflate_stored(z: &[u8]) -> Result<Vec<u8>, PngError> {
    if z.len() < 2 {
        return Err(PngError::BadZlib);
    }
    // header: CMF/FLG; (CMF * 256 + FLG) % 31 == 0 and CM == 8
    if z[0] & 0x0f != 8 || (z[0] as u32 * 256 + z[1] as u32) % 31 != 0 {
        return Err(PngError::BadZlib);
    }
    let mut i = 2;
    let mut out = Vec::new();
    loop {
        if i >= z.len() {
            return Err(PngError::BadZlib);
        }
        let h = z[i];
        i += 1;
        if h & 6 != 0 {
            return Err(PngError::Unsupported("a compressed deflate block"));
        }
        let last = h & 1 != 0;
        if i + 4 > z.len() {
            return Err(PngError::BadZlib);
        }
        let n = u16::from_le_bytes([z[i], z[i + 1]]) as usize;
        let nn = u16::from_le_bytes([z[i + 2], z[i + 3]]);
        i += 4;
        if n != (!nn) as usize {
            return Err(PngError::BadZlib);
        }
        if i + n > z.len() {
            return Err(PngError::BadZlib);
        }
        out.extend_from_slice(&z[i..i + n]);
        i += n;
        if last {
            break;
        }
    }
    Ok(out)
}

/// Compare two images of the same size. Returns `(differing pixels, bounding box of the differences, maximum
/// per-channel delta)`; the box is `None` when the images are equal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Diff {
    pub differing: usize,
    pub total: usize,
    pub max_channel: u8,
    pub bbox: Option<[i32; 4]>,
}

pub fn diff(a: &Image, b: &Image) -> Result<Diff, PngError> {
    if a.w != b.w || a.h != b.h {
        return Err(PngError::BadPixels);
    }
    let mut d = Diff {
        differing: 0,
        total: a.px.len(),
        max_channel: 0,
        bbox: None,
    };
    for y in 0..a.h {
        for x in 0..a.w {
            let (p, q) = (a.px[(y * a.w + x) as usize], b.px[(y * b.w + x) as usize]);
            if p != q {
                d.differing += 1;
                for sh in [0, 8, 16, 24] {
                    let x0 = (p >> sh & 255) as i32 - (q >> sh & 255) as i32;
                    d.max_channel = d.max_channel.max(x0.unsigned_abs() as u8);
                }
                let e = d.bbox.get_or_insert([x, y, x + 1, y + 1]);
                e[0] = e[0].min(x);
                e[1] = e[1].min(y);
                e[2] = e[2].max(x + 1);
                e[3] = e[3].max(y + 1);
            }
        }
    }
    Ok(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unsupported_dimensions_before_decoding_pixel_data() {
        let mut header = Vec::from(4096u32.to_be_bytes());
        header.extend(4096u32.to_be_bytes());
        header.extend([8, 6, 0, 0, 0]);
        let mut png = Vec::from(SIGNATURE);
        chunk(&mut png, b"IHDR", &header);
        chunk(&mut png, b"IEND", &[]);
        assert_eq!(decode(&png), Err(PngError::Unsupported("image dimensions")));
        assert_eq!(
            decode(&encode(1, 1, &[0xff123456])).unwrap().px,
            [0xff123456]
        );
    }
}
