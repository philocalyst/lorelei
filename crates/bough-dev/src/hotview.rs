//! The hot-call boundary a patched lorelei view goes through.
//!
//! A view's *variable text* is not state: it is code. Here that code is one function pointer, called on every
//! description. A patch is a small `cdylib` exporting two C symbols — `hot_text` (fill a buffer, return its
//! length) and `hot_state_token` (the state layout the code was compiled against). Loading one swaps the
//! pointer in the running process after checking the token; a different token is refused before anything runs.
//!
//! This is the smallest real version of the gpui-ce/Dioxus Subsecond route: one boundary, one dylib, no jump
//! table and no executable-memory writes. Everything else keeps the state-preserving restart path.
use std::ffi::{CString, c_char, c_void};
use std::path::Path;
use std::sync::atomic::{AtomicPtr, Ordering};

/// The ABI of the state the patch was compiled against. A patch source that exports another value is refused
/// (`hot_state_token`), which is the "changing the state type needs a restart" rule.
pub const STATE_TOKEN: u64 = 0x4465_6d6f_5374_0001; // "DemoSt" + 1

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
    let n = unsafe { f(out.as_mut_ptr(), out.len() as u32) };
    (n as usize).min(out.len())
}

/// The current text, as the view shows it.
pub fn text() -> String {
    let mut b = [0u8; 256];
    let n = text_into(&mut b);
    String::from_utf8_lossy(&b[..n]).into_owned()
}

const RTLD_NOW: i32 = 2;
const RTLD_LOCAL: i32 = 4;

unsafe extern "C" {
    fn dlopen(path: *const c_char, mode: i32) -> *mut c_void;
    fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
    fn dlerror() -> *const c_char;
    fn dlclose(handle: *mut c_void) -> i32;
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

/// Load a patch and install its `hot_text`. Refused, before any of its code runs, when its state token is not
/// the one this process was built with. The library stays loaded for the process's life (the installed pointer
/// points into it).
pub fn install(path: &Path, expected_state: u64) -> Result<(), String> {
    let c = CString::new(path.as_os_str().to_string_lossy().as_bytes())
        .map_err(|_| "the patch path contains a NUL".to_owned())?;
    // SAFETY: a NUL-terminated path and a valid mode.
    let handle = unsafe { dlopen(c.as_ptr(), RTLD_NOW | RTLD_LOCAL) };
    if handle.is_null() {
        return Err(format!("dlopen {}: {}", path.display(), dlerr()));
    }
    let token_sym = c"hot_state_token";
    // SAFETY: a loaded handle and a NUL-terminated symbol name.
    let token = unsafe { dlsym(handle, token_sym.as_ptr()) };
    if token.is_null() {
        // SAFETY: the handle came from dlopen above and is not used afterwards.
        unsafe { dlclose(handle) };
        return Err(format!(
            "{} exports no hot_state_token: {}",
            path.display(),
            dlerr()
        ));
    }
    // SAFETY: the symbol has the declared C signature (the patch's contract).
    let token: unsafe extern "C" fn() -> u64 = unsafe { core::mem::transmute(token) };
    let got = unsafe { token() };
    if got != expected_state {
        unsafe { dlclose(handle) };
        return Err(format!(
            "the patch was compiled against another state type (token {got:#x}, this process has {expected_state:#x}): a restart is needed"
        ));
    }
    let text_sym = c"hot_text";
    let f = unsafe { dlsym(handle, text_sym.as_ptr()) };
    if f.is_null() {
        unsafe { dlclose(handle) };
        return Err(format!(
            "{} exports no hot_text: {}",
            path.display(),
            dlerr()
        ));
    }
    TEXT.store(f as *mut (), Ordering::Release);
    Ok(())
}
