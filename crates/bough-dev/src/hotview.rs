//! Opt-in hot-call boundary for a trusted developer-built text patch.
use std::ffi::{CString, c_void};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::sync::atomic::{AtomicPtr, Ordering};

type TextFn = unsafe extern "C" fn(*mut u8, u32) -> u32;

unsafe extern "C" fn builtin(out: *mut u8, cap: u32) -> u32 {
    let text = b"Demo A";
    let n = text.len().min(cap as usize);
    // SAFETY: the caller owns `out` for `cap` bytes (the boundary's contract), and `n <= cap`.
    unsafe { core::ptr::copy_nonoverlapping(text.as_ptr(), out, n) };
    n as u32
}

static TEXT: AtomicPtr<()> = AtomicPtr::new(builtin as *mut ());

/// Offer the current text into `out`; returns the number of bytes written.
pub fn text_into(out: &mut [u8]) -> usize {
    let f = TEXT.load(Ordering::Acquire);
    // SAFETY: the stored value is either `builtin` or a `hot_text` symbol of the same C signature, and the
    // buffer is valid for its length.
    let f: TextFn = unsafe { core::mem::transmute::<*mut (), TextFn>(f) };
    let cap = u32::try_from(out.len()).unwrap_or(u32::MAX);
    // SAFETY: `f` implements the boundary ABI, and `cap` bytes of `out` are exclusively borrowed.
    let n = unsafe { f(out.as_mut_ptr(), cap) };
    (n as usize).min(out.len())
}

/// The current text, as the view shows it.
pub fn text() -> String {
    let mut b = [0u8; 256];
    let n = text_into(&mut b);
    String::from_utf8_lossy(&b[..n]).into_owned()
}

use libc::{dlclose, dlerror, dlopen, dlsym};

struct Library(*mut c_void);
impl Drop for Library {
    fn drop(&mut self) {
        // SAFETY: this is the non-null handle obtained from dlopen, still owned by this guard.
        unsafe { dlclose(self.0) };
    }
}

fn dlerr() -> String {
    // SAFETY: `dlerror` returns a NUL-terminated string owned by the loader (or null).
    unsafe {
        let p = dlerror();
        if p.is_null() {
            "unknown dlopen error".to_owned()
        } else {
            std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned()
        }
    }
}

/// Load a patch and install its `hot_text` when its exported state-layout tag matches.
///
/// # Safety
/// The library must be trusted native code built for this process and implement both documented C symbols
/// with the declared layouts and buffer bounds. The tag detects compatible-build mistakes. `dlopen` may
/// run initializers before tag inspection. The installed code must remain mapped for calls already in flight.
pub unsafe fn install(path: &Path, expected_state: u64) -> Result<(), String> {
    let path_c = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| "the patch path contains a NUL".to_owned())?;
    // SAFETY: the path is NUL-terminated and the caller supplies a trusted library.
    let handle = unsafe { dlopen(path_c.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
    if handle.is_null() {
        return Err(format!("dlopen {}: {}", path.display(), dlerr()));
    }
    let library = Library(handle);
    // SAFETY: the handle is live, the names are NUL-terminated, and the ABI is the caller's contract.
    let token = unsafe { dlsym(library.0, c"HOT_STATE_TOKEN".as_ptr()) }.cast::<u64>();
    if token.is_null() {
        return Err(format!(
            "{} exports no HOT_STATE_TOKEN: {}",
            path.display(),
            dlerr()
        ));
    }
    // SAFETY: HOT_STATE_TOKEN is an aligned immutable u64 in the trusted library ABI.
    let got = unsafe { token.read() };
    if got != expected_state {
        return Err(format!(
            "the patch state type changed (tag {got:#x}, current {expected_state:#x})"
        ));
    }
    // SAFETY: the caller guarantees the declared C function signature and the guard owns its mapping.
    let text = unsafe { dlsym(library.0, c"hot_text".as_ptr()) };
    if text.is_null() {
        return Err(format!(
            "{} exports no hot_text: {}",
            path.display(),
            dlerr()
        ));
    }
    // Keep old mappings as an in-flight reader may still hold the previous function pointer.
    std::mem::forget(library);
    TEXT.store(text.cast(), Ordering::Release);
    Ok(())
}
