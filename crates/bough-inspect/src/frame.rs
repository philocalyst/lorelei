//! A frame as the application paints it, and the pixel operations the tools need.
use crate::png;

#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub w: i32,
    pub h: i32,
    /// `0xAARRGGBB`, row-major.
    pub px: Vec<u32>,
}
impl Frame {
    pub fn new(w: i32, h: i32) -> Frame {
        Frame {
            w,
            h,
            px: vec![0; (w as usize) * (h as usize)],
        }
    }
    pub fn digest(&self) -> u64 {
        // FNV-1a over the words (a replay compares digests, not a cryptographic hash)
        self.px.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &p| {
            let mut h = h;
            for b in p.to_le_bytes() {
                h = (h ^ b as u64).wrapping_mul(0x100_0000_01b3);
            }
            h
        })
    }
    pub fn png(&self) -> Vec<u8> {
        let png = png::encode(self.w, self.h, &self.px);
        #[cfg(feature = "mutant_png")]
        if !self.px.is_empty() {
            // the negative control of G3: the file differs from the frame by one pixel
            png.extend_from_slice(b"mutant");
        }
        png
    }
    pub fn to_image(&self) -> png::Image {
        png::Image {
            w: self.w,
            h: self.h,
            px: self.px.clone(),
        }
    }
}
