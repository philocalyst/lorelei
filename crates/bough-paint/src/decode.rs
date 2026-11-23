//! Bounded off-thread compressed-image decode. The frame thread owns publication into the media arena.
use std::sync::Arc;
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

#[derive(Debug, Clone)]
pub struct DecodedImage {
    frames: Arc<[ImageFrame]>,
    ends_ms: Arc<[u64]>,
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
    svg: Option<[u32; 2]>,
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
                let mut svg = SvgCache::default();
                while let Ok(request) = rx.recv() {
                    let result = if let Some(size) = request.svg {
                        svg.rasterize(request.bytes, size)
                    } else {
                        native::decode(&request.bytes)
                    };
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
        self.queue(bytes, None)
    }

    /// Rasterize an SVG asset at a device size. Repeated requests on this decoder share their pixels.
    pub fn submit_svg(
        &self,
        bytes: Vec<u8>,
        width: u32,
        height: u32,
    ) -> Result<DecodeTicket, DecodeError> {
        if bytes.len() > 256 * 1024 || !(1..=2047).contains(&width) || !(1..=2047).contains(&height)
        {
            return Err(DecodeError::Limit);
        }
        self.queue(bytes, Some([width, height]))
    }

    fn queue(&self, bytes: Vec<u8>, svg: Option<[u32; 2]>) -> Result<DecodeTicket, DecodeError> {
        let (done, result) = mpsc::sync_channel(1);
        match self.work.try_send(Request { bytes, svg, done }) {
            Ok(()) => Ok(DecodeTicket(result)),
            Err(TrySendError::Full(_)) => Err(DecodeError::Busy),
            Err(TrySendError::Disconnected(_)) => Err(DecodeError::Closed),
        }
    }
}

#[derive(Default)]
struct SvgCache {
    entries: std::collections::VecDeque<(Vec<u8>, [u32; 2], DecodedImage, usize)>,
    bytes: usize,
}
impl SvgCache {
    fn rasterize(&mut self, source: Vec<u8>, size: [u32; 2]) -> ResultImage {
        const BUDGET: usize = 4 * 1024 * 1024;
        if let Some(index) = self.entries.iter().position(|(s, dimensions, _, _)| {
            s == &source && (*dimensions == size || cfg!(feature = "mutant_svg_size"))
        }) {
            let entry = self.entries.remove(index).expect("cache index existed");
            let image = entry.2.clone();
            self.entries.push_back(entry);
            return Ok(image);
        }
        let image = native::svg(&source, size)?;
        let bytes = source.capacity() + image.frames()[0].pixels.capacity() * 4;
        if bytes <= BUDGET {
            while bytes + self.bytes > BUDGET || self.entries.len() >= 64 {
                self.bytes -= self.entries.pop_front().expect("at least one entry").3;
            }
            self.bytes += bytes;
            self.entries.push_back((source, size, image.clone(), bytes));
        }
        Ok(image)
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
            let pixels = raster(image.0, width, height)?;
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
        Ok(DecodedImage {
            frames: frames.into(),
            ends_ms: ends_ms.into(),
        })
    }

    fn raster(image: Ref, width: usize, height: usize) -> Result<Vec<u32>, DecodeError> {
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
                image,
            );
            CGContextFlush(context.0);
        }
        drop(context);
        Ok(pixels)
    }

    #[link(name = "AppKit", kind = "framework")]
    unsafe extern "C" {}

    /// The opaque `CGImage` a `CGImageRef` points at, so the message's return type encodes as AppKit declares it
    /// (`^{CGImage=}`; objc2 checks the encoding in debug builds).
    #[repr(C)]
    struct CGImage {
        _opaque: [u8; 0],
    }
    // SAFETY: the encoding is the one the Objective-C runtime reports for `CGImageRef`.
    unsafe impl objc2::encode::RefEncode for CGImage {
        const ENCODING_REF: objc2::encode::Encoding = objc2::encode::Encoding::Pointer(&objc2::encode::Encoding::Struct("CGImage", &[]));
    }

    pub(super) fn svg(bytes: &[u8], [width, height]: [u32; 2]) -> ResultImage {
        use objc2::{
            class, msg_send,
            rc::{Allocated, Retained, autoreleasepool},
            runtime::AnyObject,
        };
        use objc2_foundation::{NSData, NSPoint, NSRect, NSSize};
        let text = std::str::from_utf8(bytes).map_err(|_| DecodeError::UnsupportedFormat)?;
        if !text.contains("<svg") {
            return Err(DecodeError::UnsupportedFormat);
        }
        let pixels = autoreleasepool(|_| {
            let data = NSData::with_bytes(bytes);
            // SAFETY: NSImage's public initializer accepts a live NSData. No shared AppKit
            // application is needed, and these objects stay on this decoder thread.
            let allocated: Allocated<AnyObject> = unsafe { msg_send![class!(NSImage), alloc] };
            let image: Option<Retained<AnyObject>> =
                unsafe { msg_send![allocated, initWithData: &*data] };
            let image = image.ok_or(DecodeError::Invalid)?;
            let mut bounds = NSRect::new(
                NSPoint::new(0.0, 0.0),
                NSSize::new(width as f64, height as f64),
            );
            // SAFETY: this selector belongs to NSImage. The proposed rect can be edited during the
            // message. The borrowed CGImage stays live under the NSImage and autorelease pool.
            let pixels: *const CGImage = unsafe {
                msg_send![&*image, CGImageForProposedRect: &mut bounds, context: null::<AnyObject>(), hints: null::<AnyObject>()]
            };
            let pixels = pixels as Ref;
            if pixels.is_null() {
                return Err(DecodeError::Invalid);
            }
            raster(pixels, width as usize, height as usize)
        })?;
        Ok(DecodedImage {
            frames: [ImageFrame {
                width,
                height,
                pixels,
                duration: Duration::from_millis(100),
            }]
            .into(),
            ends_ms: [100].into(),
        })
    }
}

#[cfg(not(target_os = "macos"))]
mod native {
    use super::*;
    pub(super) fn decode(_: &[u8]) -> ResultImage {
        Err(DecodeError::UnsupportedFormat)
    }
    pub(super) fn svg(_: &[u8], _: [u32; 2]) -> ResultImage {
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

#[cfg(all(test, target_os = "macos"))]
#[test]
fn svg_is_rasterized_at_the_device_size_and_shared() {
    let source = br##"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="16"><rect width="8" height="16" fill="#ff0000"/><circle cx="24" cy="8" r="6" fill="#0000ff"/></svg>"##;
    let decoder = Decoder::new().unwrap();
    let render = |w, h| {
        decoder
            .submit_svg(source.to_vec(), w, h)
            .unwrap()
            .0
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap()
    };
    let original = render(32, 16);
    let repeated = render(32, 16);
    let large = render(64, 32);
    assert!(Arc::ptr_eq(&original.frames, &repeated.frames));
    assert_eq!(
        (large.frames()[0].width, large.frames()[0].height),
        (64, 32)
    );
    assert!(!Arc::ptr_eq(&original.frames, &large.frames));
    for frame in [&original.frames()[0], &large.frames()[0]] {
        let at = |x, y| frame.pixels[(y * frame.width + x) as usize];
        assert_eq!(at(frame.width / 16, frame.height / 2), 0xffff_0000);
        assert_eq!(at(frame.width * 3 / 4, frame.height / 2), 0xff00_00ff);
        assert_eq!(at(frame.width / 2, frame.height / 2), 0);
    }
    assert_eq!(
        decoder.submit_svg(source.to_vec(), 0, 16).err(),
        Some(DecodeError::Limit)
    );
    eprintln!(
        "SVG: exact red, blue and transparent samples at two sizes, cache hit shares the BGRA frame"
    );
}
