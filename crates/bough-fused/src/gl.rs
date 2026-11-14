//! The fused presenter on OpenGL 4.3 (Linux, EGL): the same kernels as the Vulkan presenter (build.rs's GLSL form),
//! the same frame contract.
//!
//! * **The region is one persistently mapped buffer** (`ARB_buffer_storage`: `MAP_PERSISTENT | MAP_COHERENT`): the
//!   presenter allocates it and the façade carves its columns in it, so the CPU writes what the kernels read in place
//!   (GL cannot import host memory, so the region cannot be the façade's own pages as on Vulkan and Metal). The
//!   columns are read at the header's offsets (one storage binding for the whole region).
//! * **Surfaces are GL buffers, persistently mapped**: after a frame the tiles it changed (the damaged tiles and the
//!   scroll copy's rect) are copied into the window's buffer (`copy_out`), so the window's ring holds exactly what
//!   the GL surfaces hold, and the compositor gets the same damage as from the other executors.
//! * **A frame is three indirect dispatches** (sizes from the frame slot, as Metal's pre-armed frames read them) and
//!   one fence; `sync` polls the fence. No context or surface is shown: EGL's surfaceless platform (Mesa) or the
//!   default display with a surfaceless context, made current on the thread that draws.
//! * **Program binaries are cached on disk** (`$XDG_CACHE_HOME/bough`, per renderer and kernel source): a warm start
//!   compiles nothing.
//! * The context is built on another thread from process entry (`prepare_in_background`) and handed over.
use crate::frame::{cert_words, write_header, write_n_dir, Port};
use bough_frame::region::*;
use std::ffi::{c_char, c_void, CStr};

type GLenum = u32;
type GLuint = u32;
type GLint = i32;
type GLsizei = i32;
type GLbitfield = u32;
type GLsizeiptr = isize;
type GLintptr = isize;
type GLsync = *const c_void;
type EGLDisplay = *mut c_void;
type EGLContext = *mut c_void;
type EGLConfig = *mut c_void;
type EGLint = i32;

const GL_SHADER_STORAGE_BUFFER: GLenum = 0x90D2;
const GL_UNIFORM_BUFFER: GLenum = 0x8A11;
const GL_DISPATCH_INDIRECT_BUFFER: GLenum = 0x90EE;
const GL_COMPUTE_SHADER: GLenum = 0x91B9;
const GL_COMPILE_STATUS: GLenum = 0x8B81;
const GL_LINK_STATUS: GLenum = 0x8B82;
const GL_INFO_LOG_LENGTH: GLenum = 0x8B84;
const GL_MAP_READ_BIT: GLbitfield = 0x0001;
const GL_MAP_WRITE_BIT: GLbitfield = 0x0002;
const GL_MAP_PERSISTENT_BIT: GLbitfield = 0x0040;
const GL_MAP_COHERENT_BIT: GLbitfield = 0x0080;
const GL_SHADER_STORAGE_BARRIER_BIT: GLbitfield = 0x2000;
const GL_CLIENT_MAPPED_BUFFER_BARRIER_BIT: GLbitfield = 0x4000;
const GL_SYNC_GPU_COMMANDS_COMPLETE: GLenum = 0x9117;
const GL_SYNC_FLUSH_COMMANDS_BIT: GLbitfield = 0x1;
const GL_ALREADY_SIGNALED: GLenum = 0x911A;
const GL_CONDITION_SATISFIED: GLenum = 0x911C;
const GL_VENDOR: GLenum = 0x1F00;
const GL_RENDERER: GLenum = 0x1F01;
const GL_VERSION: GLenum = 0x1F02;
const GL_EXTENSIONS: GLenum = 0x1F03;
const GL_NUM_EXTENSIONS: GLenum = 0x821D;
const GL_PROGRAM_BINARY_LENGTH: GLenum = 0x8741;
const GL_PROGRAM_BINARY_RETRIEVABLE_HINT: GLenum = 0x8257;
const GL_SHADER_STORAGE_BUFFER_OFFSET_ALIGNMENT: GLenum = 0x90DF;
const GL_MAX_COMPUTE_SHADER_STORAGE_BLOCKS: GLenum = 0x90DB;
const GL_SUBGROUP_SIZE_KHR: GLenum = 0x9532;

const EGL_NONE: EGLint = 0x3038;
const EGL_EXTENSIONS: EGLint = 0x3055;
const EGL_PLATFORM_SURFACELESS_MESA: GLenum = 0x31DD;
const EGL_OPENGL_API: GLenum = 0x30A2;
const EGL_CONTEXT_MAJOR_VERSION: EGLint = 0x3098;
const EGL_CONTEXT_MINOR_VERSION: EGLint = 0x30FB;
const EGL_CONTEXT_OPENGL_PROFILE_MASK: EGLint = 0x30FD;
const EGL_CONTEXT_OPENGL_CORE_PROFILE_BIT: EGLint = 0x1;
const EGL_RENDERABLE_TYPE: EGLint = 0x3040;
const EGL_OPENGL_BIT: EGLint = 0x8;

macro_rules! table {
    ($name:ident { $($f:ident: fn($($a:ty),*) $(-> $r:ty)?;)* }) => {
        #[allow(non_snake_case)]
        struct $name { $($f: unsafe extern "C" fn($($a),*) $(-> $r)?,)* }
        impl $name {
            /// Every entry point through `get` (`None` when one is missing).
            fn load(get: &dyn Fn(&CStr) -> *const c_void) -> Result<$name, String> {
                Ok($name { $($f: {
                    let p = get(&std::ffi::CString::new(stringify!($f)).unwrap());
                    if p.is_null() { return Err(format!("missing {}", stringify!($f))); }
                    // SAFETY: the loader returned the entry point of this name, whose C signature is the one declared.
                    unsafe { std::mem::transmute::<*const c_void, unsafe extern "C" fn($($a),*) $(-> $r)?>(p) }
                },)* })
            }
        }
    };
}

table!(Egl {
    eglGetDisplay: fn(*mut c_void) -> EGLDisplay;
    eglInitialize: fn(EGLDisplay, *mut EGLint, *mut EGLint) -> u32;
    eglTerminate: fn(EGLDisplay) -> u32;
    eglQueryString: fn(EGLDisplay, EGLint) -> *const c_char;
    eglBindAPI: fn(GLenum) -> u32;
    eglChooseConfig: fn(EGLDisplay, *const EGLint, *mut EGLConfig, EGLint, *mut EGLint) -> u32;
    eglCreateContext: fn(EGLDisplay, EGLConfig, EGLContext, *const EGLint) -> EGLContext;
    eglDestroyContext: fn(EGLDisplay, EGLContext) -> u32;
    eglMakeCurrent: fn(EGLDisplay, *mut c_void, *mut c_void, EGLContext) -> u32;
    eglGetError: fn() -> EGLint;
});

table!(Gl {
    glGetString: fn(GLenum) -> *const u8;
    glGetStringi: fn(GLenum, GLuint) -> *const u8;
    glGetIntegerv: fn(GLenum, *mut GLint);
    glCreateShader: fn(GLenum) -> GLuint;
    glShaderSource: fn(GLuint, GLsizei, *const *const c_char, *const GLint);
    glCompileShader: fn(GLuint);
    glGetShaderiv: fn(GLuint, GLenum, *mut GLint);
    glGetShaderInfoLog: fn(GLuint, GLsizei, *mut GLsizei, *mut c_char);
    glDeleteShader: fn(GLuint);
    glCreateProgram: fn() -> GLuint;
    glAttachShader: fn(GLuint, GLuint);
    glLinkProgram: fn(GLuint);
    glGetProgramiv: fn(GLuint, GLenum, *mut GLint);
    glGetProgramInfoLog: fn(GLuint, GLsizei, *mut GLsizei, *mut c_char);
    glProgramParameteri: fn(GLuint, GLenum, GLint);
    glGetProgramBinary: fn(GLuint, GLsizei, *mut GLsizei, *mut GLenum, *mut c_void);
    glProgramBinary: fn(GLuint, GLenum, *const c_void, GLsizei);
    glDeleteProgram: fn(GLuint);
    glUseProgram: fn(GLuint);
    glGenBuffers: fn(GLsizei, *mut GLuint);
    glDeleteBuffers: fn(GLsizei, *const GLuint);
    glBindBuffer: fn(GLenum, GLuint);
    glBufferStorage: fn(GLenum, GLsizeiptr, *const c_void, GLbitfield);
    glMapBufferRange: fn(GLenum, GLintptr, GLsizeiptr, GLbitfield) -> *mut c_void;
    glBindBufferBase: fn(GLenum, GLuint, GLuint);
    glBindBufferRange: fn(GLenum, GLuint, GLuint, GLintptr, GLsizeiptr);
    glDispatchCompute: fn(GLuint, GLuint, GLuint);
    glDispatchComputeIndirect: fn(GLintptr);
    glMemoryBarrier: fn(GLbitfield);
    glFenceSync: fn(GLenum, GLbitfield) -> GLsync;
    glClientWaitSync: fn(GLsync, GLbitfield, u64) -> GLenum;
    glDeleteSync: fn(GLsync);
    glFlush: fn();
    glFinish: fn();
});

fn dl(names: &[&CStr]) -> *mut c_void {
    for n in names {
        // SAFETY: dlopen of a system library by name.
        let h = unsafe { libc::dlopen(n.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
        if !h.is_null() {
            return h;
        }
    }
    std::ptr::null_mut()
}

/// A GL buffer persistently mapped (or not mapped: `ptr` null).
struct GBuf {
    id: GLuint,
    ptr: *mut u8,
    size: usize,
}

/// The context and the kernels, made on another thread from process entry and handed to the drawing thread.
pub struct Prep {
    egl: Egl,
    gl: Gl,
    dpy: EGLDisplay,
    ctx: EGLContext,
    /// fs_copy, fs_bin, fs_tile.
    progs: [GLuint; 3],
    ssbo_align: usize,
    /// Renderer, version, subgroup size; whether the program binaries came from the cache.
    pub info: String,
    pub software: bool,
    pub ns: u64,
}

// SAFETY: the context is released by the thread that made it (`eglMakeCurrent` with no context) before the `Prep`
// moves; it is then current only on the thread that owns the presenter.
unsafe impl Send for Prep {}

impl Prep {
    /// An OpenGL 4.3 core context with `KHR_shader_subgroup` arithmetic and the three kernels; `Err` when EGL, the
    /// version or an extension is missing. Software renderers (llvmpipe) are reported in `software`.
    pub fn new() -> Result<Prep, String> {
        let t0 = std::time::Instant::now();
        let h = dl(&[c"libEGL.so.1", c"libEGL.so"]);
        if h.is_null() {
            return Err("no libEGL".into());
        }
        // SAFETY: dlsym on the opened library; eglGetProcAddress is EGL's own loader.
        let gpa: unsafe extern "C" fn(*const c_char) -> *const c_void = unsafe {
            let p = libc::dlsym(h, c"eglGetProcAddress".as_ptr());
            if p.is_null() {
                return Err("no eglGetProcAddress".into());
            }
            std::mem::transmute::<*mut c_void, unsafe extern "C" fn(*const c_char) -> *const c_void>(p)
        };
        let get = |n: &CStr| {
            // SAFETY: dlsym first (EGL's own entry points), then EGL's loader (GL entry points: EGL 1.5 /
            // EGL_KHR_get_all_proc_addresses).
            let p = unsafe { libc::dlsym(h, n.as_ptr()) } as *const c_void;
            if p.is_null() { unsafe { gpa(n.as_ptr()) } } else { p }
        };
        let egl = Egl::load(&get)?;
        // SAFETY: EGL calls in the documented order; every pointer argument is valid for the call.
        unsafe {
            // client extensions (EGL_EXT_client_extensions): null when the library has none
            let ce = (egl.eglQueryString)(std::ptr::null_mut(), EGL_EXTENSIONS);
            let exts = if ce.is_null() { String::new() } else { CStr::from_ptr(ce).to_string_lossy().into_owned() };
            let mut dpy: EGLDisplay = std::ptr::null_mut();
            if exts.contains("EGL_MESA_platform_surfaceless") && exts.contains("EGL_EXT_platform_base") {
                let p = get(c"eglGetPlatformDisplayEXT");
                if !p.is_null() {
                    let f = std::mem::transmute::<*const c_void, unsafe extern "C" fn(GLenum, *mut c_void, *const EGLint) -> EGLDisplay>(p);
                    dpy = f(EGL_PLATFORM_SURFACELESS_MESA, std::ptr::null_mut(), std::ptr::null());
                }
            }
            if dpy.is_null() {
                dpy = (egl.eglGetDisplay)(std::ptr::null_mut());
            }
            if dpy.is_null() {
                return Err("no EGL display".into());
            }
            let (mut maj, mut min) = (0, 0);
            if (egl.eglInitialize)(dpy, &mut maj, &mut min) == 0 {
                return Err(format!("eglInitialize: 0x{:x}", (egl.eglGetError)()));
            }
            let dexts = CStr::from_ptr((egl.eglQueryString)(dpy, EGL_EXTENSIONS)).to_string_lossy().into_owned();
            if !dexts.contains("EGL_KHR_surfaceless_context") {
                (egl.eglTerminate)(dpy);
                return Err("no EGL_KHR_surfaceless_context".into());
            }
            (egl.eglBindAPI)(EGL_OPENGL_API);
            let mut cfg: EGLConfig = std::ptr::null_mut();
            if !dexts.contains("EGL_KHR_no_config_context") && !dexts.contains("EGL_MESA_configless_context") {
                let attrs = [EGL_RENDERABLE_TYPE, EGL_OPENGL_BIT, EGL_NONE];
                let mut n = 0;
                if (egl.eglChooseConfig)(dpy, attrs.as_ptr(), &mut cfg, 1, &mut n) == 0 || n == 0 {
                    (egl.eglTerminate)(dpy);
                    return Err("no EGL config".into());
                }
            }
            let ca = [EGL_CONTEXT_MAJOR_VERSION, 4, EGL_CONTEXT_MINOR_VERSION, 3, EGL_CONTEXT_OPENGL_PROFILE_MASK, EGL_CONTEXT_OPENGL_CORE_PROFILE_BIT, EGL_NONE];
            let ctx = (egl.eglCreateContext)(dpy, cfg, std::ptr::null_mut(), ca.as_ptr());
            if ctx.is_null() {
                let e = (egl.eglGetError)();
                (egl.eglTerminate)(dpy);
                return Err(format!("no OpenGL 4.3 core context: 0x{e:x}"));
            }
            if (egl.eglMakeCurrent)(dpy, std::ptr::null_mut(), std::ptr::null_mut(), ctx) == 0 {
                (egl.eglDestroyContext)(dpy, ctx);
                (egl.eglTerminate)(dpy);
                return Err("eglMakeCurrent".into());
            }
            let gl = match Gl::load(&get) {
                Ok(g) => g,
                Err(e) => {
                    (egl.eglMakeCurrent)(dpy, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut());
                    (egl.eglDestroyContext)(dpy, ctx);
                    (egl.eglTerminate)(dpy);
                    return Err(e);
                }
            };
            let s = |e: GLenum| {
                let p = (gl.glGetString)(e);
                if p.is_null() { String::new() } else { CStr::from_ptr(p.cast()).to_string_lossy().into_owned() }
            };
            let (vendor, renderer, version) = (s(GL_VENDOR), s(GL_RENDERER), s(GL_VERSION));
            let mut n = 0;
            (gl.glGetIntegerv)(GL_NUM_EXTENSIONS, &mut n);
            let ext: Vec<String> = (0..n as u32).map(|i| CStr::from_ptr((gl.glGetStringi)(GL_EXTENSIONS, i).cast()).to_string_lossy().into_owned()).collect();
            let need = ["GL_ARB_buffer_storage", "GL_KHR_shader_subgroup"];
            let mut fail = None;
            for x in need {
                if !ext.iter().any(|e| e == x) {
                    fail = Some(format!("{renderer}: no {x}"));
                }
            }
            let mut blocks = 0;
            (gl.glGetIntegerv)(GL_MAX_COMPUTE_SHADER_STORAGE_BLOCKS, &mut blocks);
            if blocks < 10 {
                fail = Some(format!("{renderer}: {blocks} compute storage blocks (10 needed)"));
            }
            let mut p = Prep { egl, gl, dpy, ctx, progs: [0; 3], ssbo_align: 256, info: String::new(), software: false, ns: 0 };
            if let Some(f) = fail {
                drop(p);
                return Err(f);
            }
            let mut al = 0;
            (p.gl.glGetIntegerv)(GL_SHADER_STORAGE_BUFFER_OFFSET_ALIGNMENT, &mut al);
            p.ssbo_align = (al as usize).max(4);
            let mut sg = 0;
            (p.gl.glGetIntegerv)(GL_SUBGROUP_SIZE_KHR, &mut sg);
            let (progs, cached) = p.programs(&format!("{vendor}|{renderer}|{version}"))?;
            p.progs = progs;
            p.software = renderer.contains("llvmpipe") || renderer.contains("softpipe") || renderer.contains("SwiftShader");
            p.info = format!("{renderer} ({vendor}; OpenGL {version}; subgroup {sg}; programs {})", if cached { "from the cache" } else { "compiled" });
            // hand the context over: released here, made current by the presenter's thread
            (p.egl.eglMakeCurrent)(p.dpy, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut());
            p.ns = t0.elapsed().as_nanos() as u64;
            Ok(p)
        }
    }

    /// The three programs: from the binary cache when it holds this renderer's build of these sources, else compiled
    /// and linked (and cached). Returns whether they came from the cache.
    unsafe fn programs(&self, key: &str) -> Result<([GLuint; 3], bool), String> {
        let gl = &self.gl;
        let srcs = [crate::glsl::FS_COPY, crate::glsl::FS_BIN, crate::glsl::FS_TILE];
        let hash = {
            // FNV-1a over the renderer and the sources: a different driver or kernel source is a different file
            let mut h: u64 = 0xcbf29ce484222325;
            for b in key.bytes().chain(srcs.iter().flat_map(|s| s.bytes())) {
                h = (h ^ b as u64).wrapping_mul(0x100000001b3);
            }
            h
        };
        let dir = std::env::var_os("XDG_CACHE_HOME").map(std::path::PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| std::path::Path::new(&h).join(".cache"))).map(|d| d.join("bough"));
        let mut out = [0; 3];
        let mut cached = true;
        for (i, src) in srcs.iter().enumerate() {
            let path = dir.as_ref().map(|d| d.join(format!("gl-{hash:016x}-{i}.bin")));
            // SAFETY: GL calls on the current context; every buffer passed is valid for its length.
            unsafe {
                let prog = (gl.glCreateProgram)();
                let mut ok = 0;
                if let Some(b) = path.as_ref().and_then(|p| std::fs::read(p).ok()).filter(|b| b.len() > 4) {
                    let fmt = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
                    (gl.glProgramBinary)(prog, fmt, b[4..].as_ptr().cast(), (b.len() - 4) as GLsizei);
                    (gl.glGetProgramiv)(prog, GL_LINK_STATUS, &mut ok);
                }
                if ok == 0 {
                    cached = false;
                    let sh = (gl.glCreateShader)(GL_COMPUTE_SHADER);
                    let ptr = src.as_ptr() as *const c_char;
                    let len = src.len() as GLint;
                    (gl.glShaderSource)(sh, 1, &ptr, &len);
                    (gl.glCompileShader)(sh);
                    (gl.glGetShaderiv)(sh, GL_COMPILE_STATUS, &mut ok);
                    if ok == 0 {
                        let mut n = 0;
                        (gl.glGetShaderiv)(sh, GL_INFO_LOG_LENGTH, &mut n);
                        let mut log = vec![0u8; n.max(1) as usize];
                        (gl.glGetShaderInfoLog)(sh, n, std::ptr::null_mut(), log.as_mut_ptr().cast());
                        return Err(format!("kernel {i}: {}", String::from_utf8_lossy(&log)));
                    }
                    (gl.glProgramParameteri)(prog, GL_PROGRAM_BINARY_RETRIEVABLE_HINT, 1);
                    (gl.glAttachShader)(prog, sh);
                    (gl.glLinkProgram)(prog);
                    (gl.glDeleteShader)(sh);
                    (gl.glGetProgramiv)(prog, GL_LINK_STATUS, &mut ok);
                    if ok == 0 {
                        let mut n = 0;
                        (gl.glGetProgramiv)(prog, GL_INFO_LOG_LENGTH, &mut n);
                        let mut log = vec![0u8; n.max(1) as usize];
                        (gl.glGetProgramInfoLog)(prog, n, std::ptr::null_mut(), log.as_mut_ptr().cast());
                        return Err(format!("kernel {i} link: {}", String::from_utf8_lossy(&log)));
                    }
                    let mut n = 0;
                    (gl.glGetProgramiv)(prog, GL_PROGRAM_BINARY_LENGTH, &mut n);
                    if let (Some(p), true) = (&path, n > 0) {
                        let mut b = vec![0u8; n as usize + 4];
                        let mut fmt: GLenum = 0;
                        let mut got = 0;
                        (gl.glGetProgramBinary)(prog, n, &mut got, &mut fmt, b[4..].as_mut_ptr().cast());
                        if got > 0 {
                            b[..4].copy_from_slice(&fmt.to_le_bytes());
                            b.truncate(got as usize + 4);
                            if let Some(d) = p.parent() {
                                let _ = std::fs::create_dir_all(d);
                            }
                            let _ = std::fs::write(p, b);
                        }
                    }
                }
                out[i] = prog;
            }
        }
        Ok((out, cached))
    }

    unsafe fn current(&self) {
        // SAFETY: the caller's thread owns the presenter; the context is not current anywhere else.
        unsafe { (self.egl.eglMakeCurrent)(self.dpy, std::ptr::null_mut(), std::ptr::null_mut(), self.ctx) };
    }

    /// A buffer of `size` bytes, persistently mapped (coherent) when `map`.
    unsafe fn buffer(&self, size: usize, map: bool) -> GBuf {
        let gl = &self.gl;
        let size = size.max(16).next_multiple_of(16);
        let mut id = 0;
        // SAFETY: GL calls on the current context.
        unsafe {
            (gl.glGenBuffers)(1, &mut id);
            (gl.glBindBuffer)(GL_SHADER_STORAGE_BUFFER, id);
            let flags = if map { GL_MAP_READ_BIT | GL_MAP_WRITE_BIT | GL_MAP_PERSISTENT_BIT | GL_MAP_COHERENT_BIT } else { 0 };
            (gl.glBufferStorage)(GL_SHADER_STORAGE_BUFFER, size as GLsizeiptr, std::ptr::null(), flags);
            let ptr = if map { (gl.glMapBufferRange)(GL_SHADER_STORAGE_BUFFER, 0, size as GLsizeiptr, flags) as *mut u8 } else { std::ptr::null_mut() };
            assert!(!map || !ptr.is_null(), "glMapBufferRange of {size} bytes failed");
            if map {
                std::ptr::write_bytes(ptr, 0, size);
            }
            GBuf { id, ptr, size }
        }
    }

    unsafe fn free(&self, b: &GBuf) {
        // SAFETY: GL calls on the current context; deleting a mapped buffer unmaps it.
        unsafe { (self.gl.glDeleteBuffers)(1, &b.id) };
    }
}

impl Drop for Prep {
    fn drop(&mut self) {
        // SAFETY: the owning thread; the context is made current to delete what it owns, then destroyed.
        unsafe {
            self.current();
            for &p in &self.progs {
                if p != 0 {
                    (self.gl.glDeleteProgram)(p);
                }
            }
            (self.egl.eglMakeCurrent)(self.dpy, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut());
            (self.egl.eglDestroyContext)(self.dpy, self.ctx);
            (self.egl.eglTerminate)(self.dpy);
        }
    }
}

static PREP: std::sync::Mutex<Option<std::thread::JoinHandle<Result<Prep, String>>>> = std::sync::Mutex::new(None);

/// Build the context and the kernels on another thread; the next presenter takes them.
pub fn prepare_in_background() {
    let h = std::thread::Builder::new().name("bough-gl-prep".into()).spawn(Prep::new).expect("gl prep thread");
    *PREP.lock().unwrap() = Some(h);
}

pub fn take_prep() -> Result<Prep, String> {
    match PREP.lock().unwrap().take() {
        Some(h) => h.join().map_err(|_| "gl prep thread panicked".to_string())?,
        None => Prep::new(),
    }
}

#[derive(Default, Clone, Copy, Debug)]
pub struct Stats {
    pub gpu_frames: u64,
    pub tiles: u64,
    pub bin_growths: u32,
    /// Bytes copied into the window's buffers.
    pub copied: u64,
}

pub struct Fused {
    d: Prep,
    region: GBuf,
    bins: GBuf,
    counts: GBuf,
    cert: GBuf,
    atlas: GBuf,
    atlas_dims: (u32, u32),
    img: Option<GBuf>,
    ubo: GBuf,
    /// The surfaces (`w` words a row), created at first use.
    surfs: [Option<GBuf>; MAX_SURFACES],
    pub w: usize,
    pub h: usize,
    pub bin_cap: u32,
    lay: Layout,
    port: Port,
    fence: GLsync,
    last_frame: Option<(usize, usize, u32, u32)>,
    slot_flip: usize,
    pub stats: Stats,
}

impl Fused {
    /// A presenter whose region is a fresh `region_len`-byte buffer (`region()`: the façade carves its columns there).
    pub fn new(d: Prep, region_len: usize, w: usize, h: usize, bin_cap: u32) -> Fused {
        // SAFETY: the context is made current on this thread, which owns the presenter from now on.
        unsafe {
            d.current();
            let region = d.buffer(region_len, true);
            let ty = h.div_ceil(32);
            let bins = d.buffer(ty * bin_cap as usize * 16, false);
            let counts = d.buffer((ty + 1) * 4, true);
            let cert = d.buffer(MAX_SURFACES * cert_words(w, h) * 4, true);
            let atlas = d.buffer(1024 * 512, true);
            let ubo = d.buffer(3 * 256, true);
            let mut f = Fused { d, region, bins, counts, cert, atlas, atlas_dims: (1024, 512), img: None, ubo, surfs: Default::default(), w, h, bin_cap, lay: Layout::default(), port: Port::default(), fence: std::ptr::null(), last_frame: None, slot_flip: 0, stats: Stats::default() };
            f.port = f.port_now();
            f
        }
    }

    pub fn info(&self) -> &str {
        &self.d.info
    }

    pub fn software(&self) -> bool {
        self.d.software
    }

    /// The region's memory (persistently mapped, coherent), page-aligned by the driver's mapping, zeroed.
    pub fn region(&self) -> (*mut u8, usize) {
        (self.region.ptr, self.region.size)
    }

    fn port_now(&self) -> Port {
        Port {
            atlas_w: self.atlas_dims.0,
            atlas_h: self.atlas_dims.1,
            stride: self.w as u32,
            cert_plane: cert_words(self.w, self.h) as u32,
            img_w: if self.img.is_some() { IMG_W } else { 1 },
            img_h: if self.img.is_some() { IMG_H } else { 1 },
            surf_words: (self.w * self.h).max(1) as u32,
        }
    }

    fn write_port(&mut self) {
        self.port = self.port_now();
        // SAFETY: the port table inside the mapped region; the GPU is idle (the callers synced).
        unsafe { (self.region.ptr.add(FBIND_OFF) as *mut Port).write(self.port) };
    }

    pub fn set_layout(&mut self, l: Layout) {
        self.sync();
        self.lay = l;
        self.port = self.port_now();
        // the whole region is one buffer of the configured capacities: the masks are the layout's
        let m = l.masks;
        // SAFETY: the mapped region holds the header; the GPU is idle.
        unsafe { write_header(self.region.ptr, &l, [m.inst, m.page, m.glyph, m.run, m.space, m.clip, m.strike], self.bin_cap, self.port) };
        for (k, &o) in l.slot.iter().enumerate() {
            // SAFETY: the mapped uniform buffer holds 3 x 256 bytes; the GPU is idle.
            unsafe { (self.ubo.ptr.add(k * 256) as *mut [u32; 4]).write([o, 0, 0, 0]) };
        }
    }

    /// The fence of the last frame has signalled (the GPU is done with the region and the surfaces).
    fn wait(&mut self) {
        if self.fence.is_null() {
            return;
        }
        let gl = &self.d.gl;
        // SAFETY: a fence of this context, current on this thread.
        unsafe {
            let r = (gl.glClientWaitSync)(self.fence, GL_SYNC_FLUSH_COMMANDS_BIT, 5_000_000_000);
            assert!(r == GL_ALREADY_SIGNALED || r == GL_CONDITION_SATISFIED, "bough-fused: GL frame did not finish within 5 s (0x{r:x})");
            (gl.glDeleteSync)(self.fence);
        }
        self.fence = std::ptr::null();
    }

    /// Wait for the last frame; redraw it if a bin overflowed.
    pub fn sync(&mut self) {
        self.wait();
        let ty = self.lay.tiles_y() as usize;
        // SAFETY: the mapped counts hold ty + 1 words; the GPU is idle.
        let need = unsafe { *(self.counts.ptr as *const u32).add(ty) };
        if need == 0 {
            return;
        }
        // SAFETY: as above.
        unsafe { *(self.counts.ptr as *mut u32).add(ty) = 0 };
        let Some((k, _, nr, nt)) = self.last_frame else { return };
        let cap = need.next_power_of_two().max(self.bin_cap * 2);
        self.replace_bins(cap);
        self.stats.bin_growths += 1;
        // SAFETY: slot k's copy size; the GPU is idle.
        let nc = unsafe { *(self.region.ptr.add(self.lay.slot[k] as usize) as *const u32).add(8) };
        self.run(k, Some((nc, nr, nt)));
        self.sync();
    }

    fn replace_bins(&mut self, cap: u32) {
        self.wait();
        let ty = self.lay.tiles_y().max(1) as usize;
        // SAFETY: the context is current; the GPU is idle.
        unsafe {
            let b = self.d.buffer(ty * cap as usize * 16, false);
            let old = std::mem::replace(&mut self.bins, b);
            self.d.free(&old);
            (self.region.ptr as *mut u32).add(16).write(cap);
        }
        self.bin_cap = cap;
    }

    pub fn reset_bins(&mut self, cap: u32) {
        self.replace_bins(cap);
    }

    pub fn sync_atlas(&mut self, width: u32, height: u32, pixels: &[u8], dirty: bool) -> bool {
        let grown = (width, height) != self.atlas_dims;
        if !dirty && !grown {
            return false;
        }
        self.sync();
        let bytes = (width as usize * height as usize).next_multiple_of(4);
        if bytes > self.atlas.size {
            // SAFETY: the context is current; the GPU is idle.
            unsafe {
                let b = self.d.buffer(bytes, true);
                let old = std::mem::replace(&mut self.atlas, b);
                self.d.free(&old);
            }
        }
        let n = pixels.len().min(self.atlas.size);
        // SAFETY: the mapped atlas holds `size` bytes; the GPU is idle.
        unsafe { std::ptr::copy_nonoverlapping(pixels.as_ptr(), self.atlas.ptr, n) };
        if grown {
            self.atlas_dims = (width, height);
            self.write_port();
        }
        true
    }

    pub fn sync_images(&mut self, pixels: &[u8], rows: std::ops::Range<u32>) {
        self.sync();
        if self.img.is_none() {
            // SAFETY: the context is current.
            self.img = Some(unsafe { self.d.buffer(IMG_W as usize * IMG_H as usize * 4, true) });
            self.write_port();
        }
        let row = IMG_W as usize * 4;
        let (a, b) = (rows.start as usize * row, (rows.end as usize * row).min(pixels.len()));
        if a < b {
            // SAFETY: the mapped image atlas holds IMG_W * IMG_H * 4 bytes; the GPU is idle.
            unsafe { std::ptr::copy_nonoverlapping(pixels[a..b].as_ptr(), self.img.as_ref().unwrap().ptr.add(a), b - a) };
        }
    }

    fn surface(&mut self, i: usize) {
        if self.surfs[i].is_none() {
            // SAFETY: the context is current; plane `i` of the certificates is cleared (nothing known).
            unsafe {
                self.surfs[i] = Some(self.d.buffer(self.w * self.h * 4, true));
                let n = cert_words(self.w, self.h);
                std::ptr::write_bytes((self.cert.ptr as *mut u32).add(i * n), 0, n);
            }
        }
    }

    /// Bind everything and dispatch slot `k`'s frame: sizes from the slot (`None`) or given.
    fn run(&mut self, k: usize, sizes: Option<(u32, u32, u32)>) {
        let gl = &self.d.gl;
        let dummy = self.counts.id;
        // SAFETY: GL calls on the current context; every buffer bound is alive until a `wait` after this frame.
        unsafe {
            (gl.glBindBufferBase)(GL_SHADER_STORAGE_BUFFER, 0, self.region.id);
            (gl.glBindBufferBase)(GL_SHADER_STORAGE_BUFFER, 1, self.bins.id);
            (gl.glBindBufferBase)(GL_SHADER_STORAGE_BUFFER, 2, self.counts.id);
            for s in 0..MAX_SURFACES {
                (gl.glBindBufferBase)(GL_SHADER_STORAGE_BUFFER, 3 + s as u32, self.surfs[s].as_ref().map_or(dummy, |b| b.id));
            }
            (gl.glBindBufferBase)(GL_SHADER_STORAGE_BUFFER, 7, self.cert.id);
            (gl.glBindBufferBase)(GL_SHADER_STORAGE_BUFFER, 8, self.atlas.id);
            (gl.glBindBufferBase)(GL_SHADER_STORAGE_BUFFER, 9, self.img.as_ref().map_or(dummy, |b| b.id));
            (gl.glBindBufferRange)(GL_UNIFORM_BUFFER, 0, self.ubo.id, (k * 256) as GLintptr, 16);
            (gl.glBindBuffer)(GL_DISPATCH_INDIRECT_BUFFER, self.region.id);
            let slot = self.lay.slot[k] as GLintptr;
            // the scroll copy (0 workgroups without one): disjoint from the tiles the repair writes
            (gl.glUseProgram)(self.d.progs[0]);
            match sizes {
                None => (gl.glDispatchComputeIndirect)(slot + 32),
                Some((nc, _, _)) => (gl.glDispatchCompute)(nc, 1, 1),
            }
            (gl.glUseProgram)(self.d.progs[1]);
            match sizes {
                None => (gl.glDispatchComputeIndirect)(slot),
                Some((_, nr, _)) => (gl.glDispatchCompute)(nr, 1, 1),
            }
            (gl.glMemoryBarrier)(GL_SHADER_STORAGE_BARRIER_BIT);
            (gl.glUseProgram)(self.d.progs[2]);
            match sizes {
                None => (gl.glDispatchComputeIndirect)(slot + 12),
                Some((_, _, nt)) => (gl.glDispatchCompute)(nt, 1, 1),
            }
            // the pixels, certificates and counts, for the mapped reads after the fence
            (gl.glMemoryBarrier)(GL_CLIENT_MAPPED_BUFFER_BARRIER_BIT | GL_SHADER_STORAGE_BARRIER_BIT);
            self.fence = (gl.glFenceSync)(GL_SYNC_GPU_COMMANDS_COMPLETE, 0);
            (gl.glFlush)();
        }
    }

    /// Draw surface `target` (see the Vulkan presenter's `draw`). Returns the tiles repaired.
    pub fn draw(&mut self, target: usize, rows: &[u128], full: bool, n_dir: u32, copy: &SlotCopy) -> u32 {
        self.sync();
        self.surface(target);
        if !copy.is_none() && !full {
            self.surface((copy.src & 3) as usize);
        }
        self.slot_flip ^= 1;
        let k = self.slot_flip;
        // SAFETY: the mapped region is laid out by `lay`; the GPU is idle.
        let (nt, nr) = unsafe {
            write_n_dir(self.region.ptr, n_dir);
            write_slot(self.region.ptr, &self.lay, k, target as u32, rows, full, copy)
        };
        self.last_frame = Some((k, target, nr, nt));
        self.run(k, None);
        self.stats.gpu_frames += 1;
        self.stats.tiles += nt as u64;
        nt
    }

    /// After `sync`: copy what the last frame changed in surface `i` (the damaged tiles, the scroll copy's rect, or
    /// everything) into `dst`, the window's buffer `i` (`bpr` bytes a row).
    ///
    /// # Safety
    /// `dst` holds `h` rows of `bpr >= 4 w` bytes and nothing else touches it now.
    pub unsafe fn copy_out(&mut self, i: usize, dst: *mut u8, bpr: usize, rows: &[u128], full: bool, copy: &SlotCopy) {
        self.sync();
        let Some(s) = &self.surfs[i] else { return };
        let (w, h) = (self.w, self.h);
        let sbpr = w * 4;
        let mut bytes = 0u64;
        let mut rect = |x0: usize, y0: usize, x1: usize, y1: usize| {
            let (x1, y1) = (x1.min(w), y1.min(h));
            if x0 >= x1 || y0 >= y1 {
                return;
            }
            for y in y0..y1 {
                // SAFETY: both surfaces hold `h` rows of at least `4 w` bytes (the caller's contract for `dst`); the
                // GPU is done (synced).
                unsafe { std::ptr::copy_nonoverlapping(s.ptr.add(y * sbpr + x0 * 4), dst.add(y * bpr + x0 * 4), (x1 - x0) * 4) };
            }
            bytes += ((x1 - x0) * (y1 - y0) * 4) as u64;
        };
        if full {
            rect(0, 0, w, h);
        } else {
            if !copy.is_none() {
                let [x0, y0, x1, y1] = copy.rect;
                rect(x0.max(0) as usize, y0.max(0) as usize, x1.max(0) as usize, y1.max(0) as usize);
            }
            for (ty, &bits) in rows.iter().enumerate() {
                let mut b = bits;
                while b != 0 {
                    // one run of damaged tiles
                    let t0 = b.trailing_zeros() as usize;
                    let run = (b >> t0).trailing_ones() as usize;
                    rect(t0 * 32, ty * 32, (t0 + run) * 32, ty * 32 + 32);
                    b &= !(if run >= 128 { u128::MAX } else { ((1u128 << run) - 1) << t0 });
                }
            }
        }
        self.stats.copied += bytes;
    }

    /// Surface `i`'s pixels were written by something else: nothing is known (certificates cleared).
    pub fn forget(&mut self, i: usize) {
        self.sync();
        let n = cert_words(self.w, self.h);
        // SAFETY: plane `i` of the mapped certificates; the GPU is idle.
        unsafe { std::ptr::write_bytes((self.cert.ptr as *mut u32).add(i * n), 0, n) };
    }

    pub fn resize(&mut self, w: usize, h: usize) {
        if (w, h) == (self.w, self.h) {
            return;
        }
        self.sync();
        let ty = h.div_ceil(32);
        // SAFETY: the context is current; the GPU is idle; the old buffers are deleted after the new ones exist.
        unsafe {
            let bins = self.d.buffer(ty * self.bin_cap as usize * 16, false);
            let counts = self.d.buffer((ty + 1) * 4, true);
            let cert = self.d.buffer(MAX_SURFACES * cert_words(w, h) * 4, true);
            for (o, n) in [(&mut self.bins, bins), (&mut self.counts, counts), (&mut self.cert, cert)] {
                let old = std::mem::replace(o, n);
                self.d.free(&old);
            }
            for s in self.surfs.iter_mut() {
                if let Some(b) = s.take() {
                    self.d.free(&b);
                }
            }
        }
        (self.w, self.h) = (w, h);
        self.last_frame = None;
    }

    /// Surface `i`'s pixels (BGRA, `w * 4` a row).
    pub fn surface_pixels(&mut self, i: usize) -> Vec<u8> {
        self.sync();
        let n = self.w * self.h * 4;
        match &self.surfs[i] {
            // SAFETY: the mapped surface holds `n` bytes; the GPU is idle.
            Some(s) => unsafe { std::slice::from_raw_parts(s.ptr, n).to_vec() },
            None => vec![0; n],
        }
    }

    /// A from-scratch full redraw into a fresh surface (the same kernels): its BGRA bytes.
    pub fn reference_pixels(&mut self, n_dir: u32) -> Vec<u8> {
        self.sync();
        let n = cert_words(self.w, self.h);
        // the scratch takes surface 3's place and certificate plane for one frame
        let saved = self.surfs[3].take();
        // SAFETY: plane 3 of the mapped certificates; the GPU is idle.
        let cert3: Vec<u32> = unsafe { std::slice::from_raw_parts((self.cert.ptr as *const u32).add(3 * n), n).to_vec() };
        self.surface(3);
        // SAFETY: the mapped region's frame slot 2; the GPU is idle.
        let (nt, nr) = unsafe {
            write_n_dir(self.region.ptr, n_dir);
            write_slot(self.region.ptr, &self.lay, 2, 3, &[], true, &SlotCopy::NONE)
        };
        self.run(2, Some((0, nr, nt)));
        self.wait();
        let out = self.surface_pixels(3);
        let s = self.surfs[3].take().unwrap();
        // SAFETY: the context is current; the GPU is idle.
        unsafe {
            self.d.free(&s);
            std::ptr::copy_nonoverlapping(cert3.as_ptr(), (self.cert.ptr as *mut u32).add(3 * n), n);
        }
        self.surfs[3] = saved;
        out
    }

    /// Bytes of GPU buffers besides the region (bins, counts, certificates, atlases, surfaces).
    pub fn own_bytes(&self) -> u64 {
        (self.bins.size + self.counts.size + self.cert.size + self.atlas.size + self.ubo.size + self.img.as_ref().map_or(0, |b| b.size) + self.surfs.iter().flatten().map(|b| b.size).sum::<usize>()) as u64
    }
}

impl Drop for Fused {
    fn drop(&mut self) {
        self.wait();
        // SAFETY: the context is current on this thread; the GPU is idle.
        unsafe {
            for b in [&self.region, &self.bins, &self.counts, &self.cert, &self.atlas, &self.ubo] {
                self.d.free(b);
            }
            if let Some(b) = &self.img {
                self.d.free(b);
            }
            for b in self.surfs.iter().flatten() {
                self.d.free(b);
            }
            (self.d.gl.glFinish)();
        }
    }
}
