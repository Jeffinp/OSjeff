//! Native WebAssembly app engine.
//!
//! WebAssembly is OSjeff's native application format: portable programs compiled
//! to `.wasm` run *inside* the OS through this interpreter ([`wasmi`]) — no
//! foreign OS, no binary emulation, and sandboxed by construction (a guest can
//! only touch its own linear memory and the host functions we explicitly grant).
//!
//! The host surface a guest may import is the OS ABI:
//! - `host.log(ptr, len)` — write a UTF-8 string from guest memory to serial.
//! - `host.fill_rect(x, y, w, h, rgb)` — fill a rectangle in the app's surface.
//! - `host.draw_text(x, y, ptr, len, rgb, scale)` — draw guest text.
//!
//! Drawing coordinates are relative to the guest's own surface origin: the host
//! translates them by `(ox, oy)` and clips every primitive to `(cw, ch)`, so a
//! guest paints from `(0,0)` and the kernel places it inside a window's content
//! box. Console-only guests leave the surface unset, making the drawing
//! syscalls no-ops.

use crate::fb::{Canvas, Color};
use crate::sync::RacyCell;
use crate::{font, serial_print};
use bootloader_api::info::FrameBufferInfo;
use wasmi::{Caller, Engine, Extern, Instance, Linker, Module, Store};

/// The console demo (boot smoke-test) and the windowed app, assembled from WAT
/// at build time (see `build.rs`).
static DEMO_WASM: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/demo.wasm"));
static APP_WASM: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/app.wasm"));

/// Per-instance host state: the surface a guest's drawing syscalls target plus
/// the translation/clip that places that surface inside a window.
///
/// A raw pointer (not a borrow) because the guest calls back into these host
/// functions from inside `wasmi`, outliving any normal borrow; the boot/desktop
/// context is single-threaded, matching the kernel's `RacyCell` discipline.
struct HostState {
    fb: *mut u8,
    fb_len: usize,
    info: Option<FrameBufferInfo>,
    ox: i32,
    oy: i32,
    cw: i32,
    ch: i32,
}

impl HostState {
    /// Console-only state: the drawing syscalls become no-ops.
    fn console() -> Self {
        Self {
            fb: core::ptr::null_mut(),
            fb_len: 0,
            info: None,
            ox: 0,
            oy: 0,
            cw: 0,
            ch: 0,
        }
    }

    /// Build a `Canvas` over the surface, or `None` for console-only guests.
    fn canvas(&self) -> Option<Canvas<'static>> {
        let info = self.info?;
        // SAFETY: single-threaded context; the pointer/length come from a live
        // framebuffer slice that outlives this guest call (set right before it).
        let buf = unsafe { core::slice::from_raw_parts_mut(self.fb, self.fb_len) };
        Some(Canvas::new(buf, info))
    }
}

/// Unpack a packed `0xRRGGBB` integer into a framebuffer `Color`.
fn rgb(packed: i32) -> Color {
    let p = packed as u32;
    Color::rgb((p >> 16) as u8, (p >> 8) as u8, p as u8)
}

/// `host.fill_rect`: fill a guest rectangle, translated by the surface origin
/// and clipped to the surface box.
fn host_fill(st: &HostState, x: i32, y: i32, w: i32, h: i32, color: i32) {
    let Some(mut c) = st.canvas() else { return };
    let x0 = (st.ox + x).max(st.ox);
    let y0 = (st.oy + y).max(st.oy);
    let x1 = (st.ox + x + w).min(st.ox + st.cw);
    let y1 = (st.oy + y + h).min(st.oy + st.ch);
    if x1 > x0 && y1 > y0 {
        c.fill_rect(
            x0.max(0) as usize,
            y0.max(0) as usize,
            (x1 - x0) as usize,
            (y1 - y0) as usize,
            rgb(color),
        );
    }
}

/// `host.draw_text`: draw guest text translated by the surface origin. Pixels
/// outside the framebuffer are dropped by `Canvas`; the guest stays within its
/// content box by convention.
fn host_text(st: &HostState, s: &str, x: i32, y: i32, color: i32, scale: i32) {
    let Some(mut c) = st.canvas() else { return };
    font::draw_text(
        &mut c,
        (st.ox + x).max(0) as usize,
        (st.oy + y).max(0) as usize,
        s,
        rgb(color),
        scale.max(1) as usize,
    );
}

/// Register the OS ABI on `linker`. Shared by one-shot runs and the persistent
/// windowed app so every guest sees the same host surface.
fn install_abi(linker: &mut Linker<HostState>) -> Result<(), &'static str> {
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
    linker
        .func_wrap(
            "host",
            "fill_rect",
            |caller: Caller<'_, HostState>, x: i32, y: i32, w: i32, h: i32, color: i32| {
                host_fill(caller.data(), x, y, w, h, color);
            },
        )
        .map_err(|_| "link host.fill_rect")?;
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
                if let Some(s) = guest_str(&caller, ptr, len) {
                    host_text(caller.data(), s, x, y, color, scale);
                }
            },
        )
        .map_err(|_| "link host.draw_text")?;
    // host.time_ms(): milliseconds since boot. The PIT ticks at 250 Hz, so each
    // tick is 4 ms. Lets a guest animate or time its own logic.
    linker
        .func_wrap("host", "time_ms", |_: Caller<'_, HostState>| -> i64 {
            (crate::interrupts::ticks() * 4) as i64
        })
        .map_err(|_| "link host.time_ms")?;
    Ok(())
}

/// Run the embedded console demo: decode it, wire up the ABI, call `main`.
pub fn run_demo() {
    match run(DEMO_WASM, "main", HostState::console()) {
        Ok(()) => crate::serial_println!("wasm: demo module ran OK"),
        Err(e) => crate::serial_println!("wasm: demo failed: {}", e),
    }
}

/// Instantiate `bytes`, granting it the OS ABI, and call its `entry` export.
fn run(bytes: &[u8], entry: &str, state: HostState) -> Result<(), &'static str> {
    let engine = Engine::default();
    let module = Module::new(&engine, bytes).map_err(|_| "module decode")?;
    let mut store = Store::new(&engine, state);
    let mut linker = <Linker<HostState>>::new(&engine);
    install_abi(&mut linker)?;
    let instance = linker
        .instantiate_and_start(&mut store, &module)
        .map_err(|_| "instantiate")?;
    let func = instance
        .get_typed_func::<(), ()>(&store, entry)
        .map_err(|_| "no entry export")?;
    func.call(&mut store, ()).map_err(|_| "trap in entry")?;
    Ok(())
}

/// A live, persistent WASM application: instantiated once, then re-rendered each
/// frame into whichever window content box the compositor passes in.
struct App {
    store: Store<HostState>,
    instance: Instance,
}

/// The desktop's WASM app, built lazily on first paint and kept resident so the
/// interpreter is not re-instantiated every frame. Single-threaded access.
static APP: RacyCell<Option<App>> = RacyCell::new(None);

/// Build the resident windowed app from `APP_WASM`. `None` if it fails to load.
fn build_app() -> Option<App> {
    let engine = Engine::default();
    let module = Module::new(&engine, APP_WASM).ok()?;
    let mut store = Store::new(&engine, HostState::console());
    let mut linker = <Linker<HostState>>::new(&engine);
    install_abi(&mut linker).ok()?;
    let instance = linker.instantiate_and_start(&mut store, &module).ok()?;
    Some(App { store, instance })
}

/// The resident app, built lazily on first access. `None` if it fails to load.
/// SAFETY: single-threaded desktop context (compositor thread only).
fn app_mut() -> Option<&'static mut App> {
    let slot = unsafe { &mut *APP.get() };
    if slot.is_none() {
        *slot = build_app();
    }
    slot.as_mut()
}

/// Deliver a pointer event to the app in its own content-local coordinates.
/// `buttons` is a bitmask (bit 0 = left). A no-op if the app omits `on_pointer`.
pub fn on_pointer(x: i32, y: i32, buttons: i32) {
    let Some(app) = app_mut() else { return };
    if let Ok(f) = app
        .instance
        .get_typed_func::<(i32, i32, i32), ()>(&app.store, "on_pointer")
    {
        let _ = f.call(&mut app.store, (x, y, buttons));
    }
}

/// Deliver a key event to the app (`code` is an ASCII byte, or 10 for Enter).
/// A no-op if the app omits `on_key`.
pub fn on_key(code: i32) {
    let Some(app) = app_mut() else { return };
    if let Ok(f) = app
        .instance
        .get_typed_func::<i32, ()>(&app.store, "on_key")
    {
        let _ = f.call(&mut app.store, code);
    }
}

/// Render the resident WASM app into `buf` at window content box `(ox, oy, cw,
/// ch)`. The guest draws from its own origin; the host translates and clips.
/// Builds the app on first call; a load failure makes this a no-op.
pub fn draw_app(buf: &mut [u8], info: FrameBufferInfo, ox: i32, oy: i32, cw: i32, ch: i32) {
    let Some(app) = app_mut() else { return };

    {
        let st = app.store.data_mut();
        st.fb = buf.as_mut_ptr();
        st.fb_len = buf.len();
        st.info = Some(info);
        st.ox = ox;
        st.oy = oy;
        st.cw = cw;
        st.ch = ch;
    }
    if let Ok(func) = app.instance.get_typed_func::<(), ()>(&app.store, "render") {
        let _ = func.call(&mut app.store, ());
    }
    // Drop the surface so a stale pointer is never reused between frames.
    app.store.data_mut().info = None;
}

/// Borrow a UTF-8 string of `len` bytes at offset `ptr` from the guest's
/// exported `mem`. `None` on missing memory, out-of-bounds range, or non-UTF-8.
fn guest_str<'a>(caller: &'a Caller<'_, HostState>, ptr: i32, len: i32) -> Option<&'a str> {
    let Some(Extern::Memory(mem)) = caller.get_export("mem") else {
        return None;
    };
    let data = mem.data(caller);
    let (ptr, len) = (ptr as usize, len as usize);
    let bytes = data.get(ptr..ptr.saturating_add(len))?;
    core::str::from_utf8(bytes).ok()
}
