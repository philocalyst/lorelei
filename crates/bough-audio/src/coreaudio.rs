//! CoreAudio's default output unit as a sink (macOS). The same callback and the same clock as [`NullSink`]:
//! the unit's render callback hands the application's `Render` a zeroed block and stamps the clock with
//! the stream position and the host time. Output only: no capture, so no microphone permission and no
//! prompt. The unit converts from the requested format to the device's.
//!
//! [`NullSink`]: crate::NullSink
use crate::{Block, Clock, Render, Spec};
use std::ffi::c_void;

#[repr(C)]
struct ComponentDescription {
    ty: u32,
    sub: u32,
    manufacturer: u32,
    flags: u32,
    mask: u32,
}
#[repr(C)]
struct StreamDescription {
    rate: f64,
    format: u32,
    flags: u32,
    bytes_per_packet: u32,
    frames_per_packet: u32,
    bytes_per_frame: u32,
    channels: u32,
    bits: u32,
    reserved: u32,
}
#[repr(C)]
struct Buffer {
    channels: u32,
    bytes: u32,
    data: *mut c_void,
}
#[repr(C)]
struct BufferList {
    count: u32,
    first: Buffer,
}
type Proc = unsafe extern "C" fn(refcon: *mut c_void, flags: *mut u32, stamp: *const c_void, bus: u32, frames: u32, io: *mut BufferList) -> i32;
#[repr(C)]
struct CallbackStruct {
    proc_: Proc,
    refcon: *mut c_void,
}
#[repr(C)]
struct Timebase {
    numer: u32,
    denom: u32,
}

#[link(name = "AudioToolbox", kind = "framework")]
unsafe extern "C" {
    fn AudioComponentFindNext(after: *mut c_void, desc: *const ComponentDescription) -> *mut c_void;
    fn AudioComponentInstanceNew(component: *mut c_void, out: *mut *mut c_void) -> i32;
    fn AudioComponentInstanceDispose(unit: *mut c_void) -> i32;
    fn AudioUnitSetProperty(unit: *mut c_void, id: u32, scope: u32, element: u32, data: *const c_void, size: u32) -> i32;
    fn AudioUnitGetProperty(unit: *mut c_void, id: u32, scope: u32, element: u32, data: *mut c_void, size: *mut u32) -> i32;
    fn AudioUnitInitialize(unit: *mut c_void) -> i32;
    fn AudioUnitUninitialize(unit: *mut c_void) -> i32;
    fn AudioOutputUnitStart(unit: *mut c_void) -> i32;
    fn AudioOutputUnitStop(unit: *mut c_void) -> i32;
}
unsafe extern "C" {
    fn mach_absolute_time() -> u64;
    fn mach_timebase_info(info: *mut Timebase) -> i32;
}

const fn fourcc(s: &[u8; 4]) -> u32 {
    (s[0] as u32) << 24 | (s[1] as u32) << 16 | (s[2] as u32) << 8 | s[3] as u32
}

/// The host clock in nanoseconds (the timebase CoreAudio's stamps and the display's are in).
pub fn host_now_ns() -> u64 {
    let mut tb = Timebase { numer: 0, denom: 0 };
    // SAFETY: both calls only write the struct passed / read the clock.
    let t = unsafe {
        mach_timebase_info(&mut tb);
        mach_absolute_time()
    };
    (t as u128 * tb.numer.max(1) as u128 / tb.denom.max(1) as u128) as u64
}

struct State {
    render: Box<dyn Render>,
    clock: Clock,
    spec: Spec,
    latency: u32,
    position: u64,
}

/// A running output. Dropping it stops the unit before the callback's state is freed.
pub struct Output {
    unit: *mut c_void,
    state: *mut State,
    clock: Clock,
    pub spec: Spec,
    /// The unit's reported latency, frames.
    pub latency: u32,
}
// SAFETY: the unit is an opaque handle CoreAudio lets any thread stop and dispose; `state` is touched by
// the render thread only while the unit runs and by `drop` only after it has stopped.
unsafe impl Send for Output {}

unsafe extern "C" fn render_proc(refcon: *mut c_void, _flags: *mut u32, _stamp: *const c_void, _bus: u32, frames: u32, io: *mut BufferList) -> i32 {
    // SAFETY: `refcon` is the `State` this output leaked in `start` and frees only after the unit stopped;
    // CoreAudio calls this from one render thread at a time. `io` is the unit's buffer list for this call:
    // one interleaved buffer (the format `start` set), valid for `bytes` bytes.
    unsafe {
        let st = &mut *(refcon as *mut State);
        let buf = &mut (*io).first;
        let n = (buf.bytes as usize / 4).min(frames as usize * st.spec.channels as usize);
        if buf.data.is_null() || n == 0 {
            return 0;
        }
        let samples = std::slice::from_raw_parts_mut(buf.data as *mut f32, n);
        samples.fill(0.0);
        st.render.render(&mut Block { samples, spec: st.spec, position: st.position, latency: st.latency });
        st.position += frames as u64;
        st.clock.stamp(st.position, host_now_ns(), st.spec, st.latency, frames);
    }
    0
}

impl Output {
    /// Open the default output and start calling `render`. `None` when there is no default output or it
    /// refuses any step (the error code is printed to stderr: there is nothing a caller can do with it but
    /// fall back to no sound).
    pub fn start(spec: Spec, render: impl Render + 'static) -> Option<Output> {
        let desc = ComponentDescription { ty: fourcc(b"auou"), sub: fourcc(b"def "), manufacturer: fourcc(b"appl"), flags: 0, mask: 0 };
        let clock = Clock::new();
        // SAFETY: plain C calls with pointers to live locals of the declared layouts; every status is
        // checked, and the unit is disposed on each failure path before returning.
        unsafe {
            let comp = AudioComponentFindNext(std::ptr::null_mut(), &desc);
            if comp.is_null() {
                return None;
            }
            let mut unit = std::ptr::null_mut();
            if AudioComponentInstanceNew(comp, &mut unit) != 0 || unit.is_null() {
                return None;
            }
            let fail = |unit: *mut c_void, what: &str, e: i32| -> Option<Output> {
                eprintln!("bough-audio: coreaudio {what}: {e}");
                AudioComponentInstanceDispose(unit);
                None
            };
            let bytes = 4 * spec.channels as u32;
            // linear PCM, float (1) | packed (8), interleaved
            let fmt = StreamDescription { rate: spec.rate as f64, format: fourcc(b"lpcm"), flags: 1 | 8, bytes_per_packet: bytes, frames_per_packet: 1, bytes_per_frame: bytes, channels: spec.channels as u32, bits: 32, reserved: 0 };
            // kAudioUnitProperty_StreamFormat = 8 on the input scope (1) of the output element (0)
            let e = AudioUnitSetProperty(unit, 8, 1, 0, &fmt as *const _ as *const c_void, size_of::<StreamDescription>() as u32);
            if e != 0 {
                return fail(unit, "stream format", e);
            }
            // kAudioUnitProperty_Latency = 12 (seconds), global scope
            let (mut secs, mut size) = (0f64, 8u32);
            let latency = if AudioUnitGetProperty(unit, 12, 0, 0, &mut secs as *mut f64 as *mut c_void, &mut size) == 0 { (secs * spec.rate as f64) as u32 } else { 0 };
            let state = Box::into_raw(Box::new(State { render: Box::new(render), clock: clock.clone(), spec, latency, position: 0 }));
            let cb = CallbackStruct { proc_: render_proc, refcon: state as *mut c_void };
            // kAudioUnitProperty_SetRenderCallback = 23
            let mut e = AudioUnitSetProperty(unit, 23, 1, 0, &cb as *const _ as *const c_void, size_of::<CallbackStruct>() as u32);
            if e == 0 {
                e = AudioUnitInitialize(unit);
            }
            if e == 0 {
                e = AudioOutputUnitStart(unit);
                if e != 0 {
                    AudioUnitUninitialize(unit);
                }
            }
            if e != 0 {
                drop(Box::from_raw(state));
                return fail(unit, "start", e);
            }
            Some(Output { unit, state, clock, spec, latency })
        }
    }
    pub fn clock(&self) -> Clock {
        self.clock.clone()
    }
}
impl Drop for Output {
    fn drop(&mut self) {
        // SAFETY: stopping returns once no render call is in flight (AudioOutputUnitStop is synchronous for
        // the HAL output unit), so the state is freed with no reader; the handles are this output's own.
        unsafe {
            AudioOutputUnitStop(self.unit);
            AudioUnitUninitialize(self.unit);
            AudioComponentInstanceDispose(self.unit);
            drop(Box::from_raw(self.state));
        }
    }
}
