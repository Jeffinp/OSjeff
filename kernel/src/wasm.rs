//! Native WebAssembly app engine.
//!
//! WebAssembly is OSjeff's native application format: portable programs compiled
//! to `.wasm` run *inside* the OS through this interpreter ([`wasmi`]) — no
//! foreign OS, no binary emulation, and sandboxed by construction (a guest can
//! only touch its own linear memory and the host functions we explicitly grant).
//!
//! The host surface a guest may import is the OS ABI:
//! - `host.log(ptr, len)` — write a UTF-8 string from guest memory to serial.
//! - `host.fill_rect(x, y, w, h, rgb)` — fill a framebuffer rectangle.
//! - `host.draw_text(x, y, ptr, len, rgb, scale)` — draw guest text.
//!
//! Input and timing syscalls (and per-window app surfaces) come next.

use crate::fb::{Canvas, Color};
use crate::{font, serial_print};
use bootloader_api::info::FrameBufferInfo;
use wasmi::{Caller, Engine, Extern, Linker, Module, Store};

/// The console demo (logs a greeting) and the GUI demo (paints a panel),
/// assembled from WAT at build time (see `build.rs`).
static DEMO_WASM: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/demo.wasm"));
static GUI_WASM: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/gui.wasm"));

/// Per-instance host state: the framebuffer a guest's drawing syscalls target.
/// A raw pointer (not a borrow) because the guest calls back into these host
/// functions from inside `wasmi`, outliving any normal borrow; the boot context
/// is single-threaded, matching the kernel's `RacyCell` discipline. `info` is
/// `None` for console-only guests, which makes the drawing syscalls no-ops.
struct HostState {
    fb: *mut u8,
    fb_len: usize,
    info: Option<FrameBufferInfo>,
}

impl HostState {
    /// Console-only state: drawing syscalls become no-ops.
    fn console() -> Self {
        Self {
            fb: core::ptr::null_mut(),
            fb_len: 0,
            info: None,
        }
    }

    /// Grant a guest pixel access to `buf` with the given framebuffer layout.
    fn with_fb(buf: &mut [u8], info: FrameBufferInfo) -> Self {
        Self {
            fb: buf.as_mut_ptr(),
            fb_len: buf.len(),
            info: Some(info),
        }
    }

    /// Borrow the framebuffer as a `Canvas`, or `None` for console-only guests.
    fn canvas(&self) -> Option<Canvas<'_>> {
        let info = self.info?;
        // SAFETY: single-threaded boot context; the pointer/length come from the
        // live framebuffer slice that outlives this guest call.
        let buf = unsafe { core::slice::from_raw_parts_mut(self.fb, self.fb_len) };
        Some(Canvas::new(buf, info))
    }
}

/// Unpack a packed `0xRRGGBB` integer into a framebuffer `Color`.
fn rgb(packed: i32) -> Color {
    let p = packed as u32;
    Color::rgb((p >> 16) as u8, (p >> 8) as u8, p as u8)
}

/// Run the embedded console demo: decode it, wire up the host ABI, call `main`.
pub fn run_demo() {
    match run(DEMO_WASM, "main", HostState::console()) {
        Ok(()) => crate::serial_println!("wasm: demo module ran OK"),
        Err(e) => crate::serial_println!("wasm: demo failed: {}", e),
    }
}

/// Run the embedded GUI demo, letting it paint into `buf` (a live framebuffer
/// layer) through the drawing ABI. Used to render the WASM panel onto the
/// desktop background, proving guest→framebuffer drawing end to end.
pub fn run_gui_demo(buf: &mut [u8], info: FrameBufferInfo) {
    match run(GUI_WASM, "render", HostState::with_fb(buf, info)) {
        Ok(()) => crate::serial_println!("wasm: gui module rendered OK"),
        Err(e) => crate::serial_println!("wasm: gui failed: {}", e),
    }
}

/// Instantiate `bytes` as a WebAssembly module, granting it the OS ABI, and call
/// its `entry` export. Returns a short error string on any failure.
fn run(bytes: &[u8], entry: &str, state: HostState) -> Result<(), &'static str> {
    let engine = Engine::default();
    let module = Module::new(&engine, bytes).map_err(|_| "module decode")?;

    let mut store = Store::new(&engine, state);
    let mut linker = <Linker<HostState>>::new(&engine);

    // host.log(ptr, len): print a UTF-8 string from the guest's linear memory.
    linker
        .func_wrap(
            "host",
            "log",
            |caller: Caller<'_, HostState>, ptr: i32, len: i32| {
                if let Some(s) = guest_str(&caller, ptr, len) {
                    serial_print!("{}", s);
                }
            },
        )
        .map_err(|_| "link host.log")?;

    // host.fill_rect(x, y, w, h, rgb): fill a rectangle in the framebuffer.
    linker
        .func_wrap(
            "host",
            "fill_rect",
            |caller: Caller<'_, HostState>, x: i32, y: i32, w: i32, h: i32, color: i32| {
                if let Some(mut c) = caller.data().canvas() {
                    c.fill_rect(
                        x.max(0) as usize,
                        y.max(0) as usize,
                        w.max(0) as usize,
                        h.max(0) as usize,
                        rgb(color),
                    );
                }
            },
        )
        .map_err(|_| "link host.fill_rect")?;

    // host.draw_text(x, y, ptr, len, rgb, scale): draw guest text at scale.
    linker
        .func_wrap(
            "host",
            "draw_text",
            |caller: Caller<'_, HostState>,
             x: i32,
             y: i32,
             ptr: i32,
             len: i32,
             color: i32,
             scale: i32| {
                if let Some(s) = guest_str(&caller, ptr, len)
                    && let Some(mut c) = caller.data().canvas()
                {
                    font::draw_text(
                        &mut c,
                        x.max(0) as usize,
                        y.max(0) as usize,
                        s,
                        rgb(color),
                        (scale.max(1)) as usize,
                    );
                }
            },
        )
        .map_err(|_| "link host.draw_text")?;

    let instance = linker
        .instantiate_and_start(&mut store, &module)
        .map_err(|_| "instantiate")?;

    let func = instance
        .get_typed_func::<(), ()>(&store, entry)
        .map_err(|_| "no entry export")?;

    func.call(&mut store, ()).map_err(|_| "trap in entry")?;
    Ok(())
}

/// Borrow a UTF-8 string of `len` bytes at offset `ptr` from the guest's
/// exported `mem`. `None` if there is no memory, the range is out of bounds, or
/// the bytes are not valid UTF-8.
fn guest_str<'a>(caller: &'a Caller<'_, HostState>, ptr: i32, len: i32) -> Option<&'a str> {
    let Some(Extern::Memory(mem)) = caller.get_export("mem") else {
        return None;
    };
    let data = mem.data(caller);
    let (ptr, len) = (ptr as usize, len as usize);
    let bytes = data.get(ptr..ptr.saturating_add(len))?;
    core::str::from_utf8(bytes).ok()
}
