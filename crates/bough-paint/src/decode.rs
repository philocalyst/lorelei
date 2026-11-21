//! Bounded off-thread compressed-image decode. The frame thread owns publication into the media arena.
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError};
use std::time::Duration;

const MAX_INPUT: usize = 16 * 1024 * 1024;
const MAX_FRAMES: usize = 256;
const MAX_PIXELS: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    UnsupportedFormat,
    Limit,
    Invalid,
    Busy,
    Closed,
}

#[derive(Debug)]
pub struct ImageFrame {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u32>,
    pub duration: Duration,
}

#[derive(Debug)]
pub struct DecodedImage {
    frames: Vec<ImageFrame>,
    ends_ms: Vec<u64>,
}
impl DecodedImage {
    pub fn frames(&self) -> &[ImageFrame] {
        &self.frames
    }
    /// The frame for an absolute elapsed instant. An image with multiple frames repeats its decoded sequence.
    pub fn frame_at(&self, elapsed: Duration) -> (usize, &ImageFrame) {
        let period = *self.ends_ms.last().expect("decoded images have a frame");
        let tick = (elapsed.as_millis() % u128::from(period)) as u64;
        let i = self.ends_ms.partition_point(|&end| end <= tick);
        (i, &self.frames[i])
    }
}

type ResultImage = Result<DecodedImage, DecodeError>;
struct Request {
    bytes: Vec<u8>,
    done: SyncSender<ResultImage>,
}

/// A bounded queue with one named decoder thread. Submit and poll do bounded work on the caller.
pub struct Decoder {
    work: SyncSender<Request>,
}

pub struct DecodeTicket(Receiver<ResultImage>);
impl DecodeTicket {
    pub fn poll(&self) -> Result<Option<DecodedImage>, DecodeError> {
        match self.0.try_recv() {
            Ok(result) => result.map(Some),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(DecodeError::Closed),
        }
    }
}

impl Decoder {
    pub fn new() -> std::io::Result<Self> {
        let (work, rx) = mpsc::sync_channel::<Request>(4);
        std::thread::Builder::new()
            .name("bough-image-decoder".into())
            .spawn(move || {
                while let Ok(request) = rx.recv() {
                    let result = native::decode(&request.bytes);
                    // This ticket's single result cannot fill the channel before this send.
                    let _ = request.done.try_send(result);
                }
            })?;
        Ok(Self { work })
    }

    pub fn submit(&self, bytes: Vec<u8>) -> Result<DecodeTicket, DecodeError> {
        if bytes.len() > MAX_INPUT {
            return Err(DecodeError::Limit);
        }
        if !(bytes.starts_with(b"\x89PNG\r\n\x1a\n")
            || bytes.starts_with(b"\xff\xd8\xff")
            || (bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP")))
        {
            return Err(DecodeError::UnsupportedFormat);
        }
        let (done, result) = mpsc::sync_channel(1);
        match self.work.try_send(Request { bytes, done }) {
            Ok(()) => Ok(DecodeTicket(result)),
            Err(TrySendError::Full(_)) => Err(DecodeError::Busy),
            Err(TrySendError::Disconnected(_)) => Err(DecodeError::Closed),
        }
    }
}

#[cfg(target_os = "macos")]
mod native {
    use super::*;
    use std::ffi::c_void;
    use std::ptr::null;
    type Ref = *const c_void;

    #[repr(C)]
    struct Rect {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFDataCreate(allocator: Ref, bytes: *const u8, len: isize) -> Ref;
        fn CFRelease(value: Ref);
        fn CFDictionaryGetValue(dictionary: Ref, key: Ref) -> Ref;
        fn CFGetTypeID(value: Ref) -> usize;
        fn CFNumberGetTypeID() -> usize;
        fn CFDictionaryGetTypeID() -> usize;
        fn CFNumberGetValue(number: Ref, kind: isize, result: *mut c_void) -> bool;
    }
    #[link(name = "ImageIO", kind = "framework")]
    unsafe extern "C" {
        fn CGImageSourceCreateWithData(data: Ref, options: Ref) -> Ref;
        fn CGImageSourceGetCount(source: Ref) -> usize;
        fn CGImageSourceCopyPropertiesAtIndex(source: Ref, index: usize, options: Ref) -> Ref;
        fn CGImageSourceCreateImageAtIndex(source: Ref, index: usize, options: Ref) -> Ref;
        static kCGImagePropertyPixelWidth: Ref;
        static kCGImagePropertyPixelHeight: Ref;
        static kCGImagePropertyPNGDictionary: Ref;
        static kCGImagePropertyAPNGUnclampedDelayTime: Ref;
        static kCGImagePropertyAPNGDelayTime: Ref;
        static kCGImagePropertyWebPDictionary: Ref;
        static kCGImagePropertyWebPUnclampedDelayTime: Ref;
        static kCGImagePropertyWebPDelayTime: Ref;
    }
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGImageGetWidth(image: Ref) -> usize;
        fn CGImageGetHeight(image: Ref) -> usize;
        fn CGColorSpaceCreateWithName(name: Ref) -> Ref;
        static kCGColorSpaceSRGB: Ref;
        fn CGBitmapContextCreate(
            data: *mut c_void,
            width: usize,
            height: usize,
            bits: usize,
            stride: usize,
            colorspace: Ref,
            info: u32,
        ) -> Ref;
        fn CGContextSetBlendMode(context: Ref, mode: i32);
        fn CGContextDrawImage(context: Ref, rect: Rect, image: Ref);
        fn CGContextFlush(context: Ref);
    }

    struct Owned(Ref);
    impl Owned {
        fn from_created(value: Ref) -> Result<Self, DecodeError> {
            if value.is_null() {
                Err(DecodeError::Invalid)
            } else {
                Ok(Self(value))
            }
        }
    }
    impl Drop for Owned {
        fn drop(&mut self) {
            // SAFETY: only non-null values returned by successful Create/Copy rules enter Owned.
            unsafe { CFRelease(self.0) }
        }
    }

    fn number(dict: Ref, key: Ref) -> Option<f64> {
        // SAFETY: dict and key are live CF references supplied by ImageIO. The dictionary owns the
        // returned reference. Check its type before passing it to CFNumberGetValue.
        unsafe {
            let value = CFDictionaryGetValue(dict, key);
            if value.is_null() || CFGetTypeID(value) != CFNumberGetTypeID() {
                return None;
            }
            let mut out = 0.0f64;
            CFNumberGetValue(value, 6, (&mut out as *mut f64).cast()).then_some(out)
        }
    }

    fn delay(properties: Ref, webp: bool) -> u64 {
        // SAFETY: properties is retained during the call. ImageIO owns the constant keys. The
        // dictionary result is borrowed and checked before the subsequent lookups.
        unsafe {
            let (dict, unclamped, clamped) = if webp {
                (
                    kCGImagePropertyWebPDictionary,
                    kCGImagePropertyWebPUnclampedDelayTime,
                    kCGImagePropertyWebPDelayTime,
                )
            } else {
                (
                    kCGImagePropertyPNGDictionary,
                    kCGImagePropertyAPNGUnclampedDelayTime,
                    kCGImagePropertyAPNGDelayTime,
                )
            };
            let format = CFDictionaryGetValue(properties, dict);
            if format.is_null() || CFGetTypeID(format) != CFDictionaryGetTypeID() {
                return 100;
            }
            let elapsed = number(format, unclamped).or_else(|| number(format, clamped));
            elapsed
                .filter(|s| s.is_finite() && *s > 0.0)
                .map_or(100, |s| ((s * 1000.0).round() as u64).clamp(1, 60_000))
        }
    }

    pub(super) fn decode(bytes: &[u8]) -> ResultImage {
        // SAFETY: bytes is a live Rust slice and the admission limit bounds its signed length.
        // CFDataCreate copies the bytes. Null selects the platform allocator/options.
        let data = Owned::from_created(unsafe {
            CFDataCreate(null(), bytes.as_ptr(), bytes.len() as isize)
        })?;
        // SAFETY: data is retained while ImageIO constructs its source, and the options are null.
        let source = Owned::from_created(unsafe { CGImageSourceCreateWithData(data.0, null()) })?;
        // SAFETY: source is retained from CGImageSourceCreateWithData.
        let count = unsafe { CGImageSourceGetCount(source.0) };
        if count == 0 {
            return Err(DecodeError::Invalid);
        }
        if count > MAX_FRAMES {
            return Err(DecodeError::Limit);
        }
        let mut frames = Vec::with_capacity(count);
        let mut ends_ms = Vec::with_capacity(count);
        let mut total = 0usize;
        let mut end = 0u64;
        for i in 0..count {
            if cfg!(feature = "mutant_drop_image_frame") && i > 0 {
                break;
            }
            // SAFETY: i is below the native frame count and source remains retained.
            let props = Owned::from_created(unsafe {
                CGImageSourceCopyPropertiesAtIndex(source.0, i, null())
            })?;
            // SAFETY: the properties dictionary and ImageIO's constant keys are live.
            let (w, h) = unsafe {
                (
                    number(props.0, kCGImagePropertyPixelWidth),
                    number(props.0, kCGImagePropertyPixelHeight),
                )
            };
            let dimension = |v: Option<f64>| -> Result<usize, DecodeError> {
                let n = v.ok_or(DecodeError::Invalid)?;
                if n.fract() != 0.0 || !(1.0..=2047.0).contains(&n) {
                    return Err(DecodeError::Limit);
                }
                Ok(n as usize)
            };
            let (width, height) = (dimension(w)?, dimension(h)?);
            total += width * height;
            if total > MAX_PIXELS {
                return Err(DecodeError::Limit);
            }
            // SAFETY: the frame index and its bounded dimensions were checked via the same source.
            let image = Owned::from_created(unsafe {
                CGImageSourceCreateImageAtIndex(source.0, i, null())
            })?;
            // SAFETY: image is a live CGImageRef. Confirm metadata matches actual decoded extents.
            if unsafe { (CGImageGetWidth(image.0), CGImageGetHeight(image.0)) } != (width, height) {
                return Err(DecodeError::Invalid);
            }
            let mut pixels = vec![0u32; width * height];
            // SAFETY: kCGColorSpaceSRGB is an immutable CoreGraphics string reference.
            let colorspace =
                Owned::from_created(unsafe { CGColorSpaceCreateWithName(kCGColorSpaceSRGB) })?;
            // SAFETY: each row holds width * 4 bytes and `pixels` is exclusively available to the
            // context until it is dropped. 0x2002 selects little-endian premultiplied alpha first.
            let context = Owned::from_created(unsafe {
                CGBitmapContextCreate(
                    pixels.as_mut_ptr().cast(),
                    width,
                    height,
                    8,
                    width * 4,
                    colorspace.0,
                    0x2002,
                )
            })?;
            // SAFETY: both CoreGraphics objects are retained and context owns a valid bitmap target.
            unsafe {
                CGContextSetBlendMode(context.0, 17); // kCGBlendModeCopy
                CGContextDrawImage(
                    context.0,
                    Rect {
                        x: 0.0,
                        y: 0.0,
                        width: width as f64,
                        height: height as f64,
                    },
                    image.0,
                );
                CGContextFlush(context.0);
            }
            drop(context);
            let ms = delay(props.0, bytes.starts_with(b"RIFF"));
            frames.push(ImageFrame {
                width: width as u32,
                height: height as u32,
                pixels,
                duration: Duration::from_millis(ms),
            });
            end += ms;
            ends_ms.push(end);
        }
        Ok(DecodedImage { frames, ends_ms })
    }
}

#[cfg(not(target_os = "macos"))]
mod native {
    use super::*;
    pub(super) fn decode(_: &[u8]) -> ResultImage {
        Err(DecodeError::UnsupportedFormat)
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn stills_and_animation_decode_to_independent_pixels() {
        let decoder = Decoder::new().unwrap();
        let decode = |data: &[u8]| {
            let ticket = decoder.submit(data.to_vec()).unwrap();
            ticket
                .0
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .unwrap()
        };
        assert_eq!(
            decoder.submit(vec![0, 1, 2]).err(),
            Some(DecodeError::UnsupportedFormat)
        );
        assert_eq!(
            decoder.submit(vec![0; MAX_INPUT + 1]).err(),
            Some(DecodeError::Limit)
        );
        let invalid = decoder.submit(b"\x89PNG\r\n\x1a\n".to_vec()).unwrap();
        assert_eq!(
            invalid
                .0
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .err(),
            Some(DecodeError::Invalid)
        );
        let png = decode(include_bytes!("../tests/fixtures/decode/pixels.png"));
        let webp = decode(include_bytes!("../tests/fixtures/decode/pixels.webp"));
        let pixels = [0xfff0_2010, 0x8000_4020, 0xff0a_28e6, 0];
        for image in [&png, &webp] {
            let [frame] = image.frames() else {
                panic!("one still frame must decode")
            };
            assert_eq!((frame.width, frame.height), (2, 2));
            assert_eq!(&frame.pixels[..], &pixels);
        }
        let jpeg = decode(include_bytes!("../tests/fixtures/decode/solid.jpg"));
        let [frame] = jpeg.frames() else {
            panic!("one JPEG frame must decode")
        };
        assert_eq!((frame.width, frame.height), (16, 16));
        for &p in &frame.pixels {
            assert_eq!(p >> 24, 255);
            for (shift, expected) in [(16, 220), (8, 53), (0, 24)] {
                assert!(((p >> shift) & 255).abs_diff(expected) <= 4, "{p:#010x}");
            }
        }
        let animated = decode(include_bytes!("../tests/fixtures/decode/frames.png"));
        assert_eq!(
            animated.frames().len(),
            2,
            "both animation frames must decode"
        );
        assert_eq!(&animated.frames()[0].pixels[..], &pixels);
        assert_eq!(animated.frames()[1].pixels[0], 0xff10_40ed);
        assert_eq!(animated.frames()[0].duration, Duration::from_millis(73));
        assert_eq!(animated.frames()[1].duration, Duration::from_millis(121));
        assert_eq!(
            [0, 72, 73, 193, 194, 267].map(|ms| animated.frame_at(Duration::from_millis(ms)).0),
            [0, 0, 1, 1, 0, 1]
        );
        eprintln!(
            "decoder: PNG and WebP exact 4-word pixels, JPEG 256 pixels in tolerance, APNG 2 frames at 73/121 ms"
        );
    }
}
