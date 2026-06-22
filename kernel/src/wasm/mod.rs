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
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use wasmi::{Caller, Engine, Extern, Instance, Linker, Module, Store};

/// The console demo (boot smoke-test) and the windowed app, assembled from WAT
/// at build time (see `build.rs`).
static DEMO_WASM: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/demo.wasm"));
static APP_WASM: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/app.wasm"));
/// The DOOM IWAD, embedded so the WASI file layer ([`wasi`]) can serve it to a
/// wasm guest. Empty unless the kernel was built in DOOM mode (`build.rs` writes
/// the real `doom1.wad` to `OUT_DIR` then, otherwise an empty placeholder).
pub(super) static WAD: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/doom1.wad"));

mod wasi;

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
    /// WASI: the single open WAD file handle (DOOM keeps the IWAD open for the
    /// whole session). `-1` when closed. `wad_pos` is the read cursor.
    wad_fd: i32,
    wad_pos: usize,
    /// Seed for `random_get` (reseeded from the clock on first use).
    rng: u32,
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
            wad_fd: -1,
            wad_pos: 0,
            rng: 0,
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

/// `host.draw_text`: draw guest text translated by the surface origin and
/// clipped to the content box, glyph by glyph, so a guest can never paint text
/// past its window onto the desktop or another window (sandbox containment).
fn host_text(st: &HostState, s: &str, x: i32, y: i32, color: i32, scale: i32) {
    let Some(mut c) = st.canvas() else { return };
    let scale = scale.max(1) as usize;
    let cw = font::cell_w(scale) as i32;
    let gh = (8 * scale) as i32; // glyph cell height
    let (bx, by, bw, bh) = (st.ox, st.oy, st.cw, st.ch);
    let py = st.oy + y;
    // Vertical clip: drop the whole line unless it fits inside the box.
    if py < by || py + gh > by + bh {
        return;
    }
    let col = rgb(color);
    let mut px = st.ox + x;
    for &ch in s.as_bytes() {
        if px >= bx + bw {
            break; // past the right edge — nothing more is visible
        }
        // Only draw glyphs wholly inside the box horizontally.
        if px >= bx && px + cw <= bx + bw {
            font::draw_char(&mut c, px as usize, py as usize, ch, col, scale);
        }
        px += cw;
    }
}

/// `host.blit`: copy a `w*h` RGBA image from guest memory (at offset `off`) to
/// the surface at content-local `(dx, dy)`, translated by the surface origin and
/// clipped to the content box. The per-frame primitive for framebuffer apps
/// (a game pushes its rendered frame this way).
fn host_blit(caller: &Caller<'_, HostState>, off: i32, w: i32, h: i32, dx: i32, dy: i32) {
    if w <= 0 || h <= 0 {
        return;
    }
    let st = caller.data();
    let (ox, oy, cw, ch) = (st.ox, st.oy, st.cw, st.ch);
    let Some(mut c) = st.canvas() else { return };
    let need = (w as i64) * (h as i64) * 4;
    let Some(px) = guest_bytes(caller, off, need.min(i32::MAX as i64) as i32) else {
        return;
    };
    // Integer nearest-neighbor upscale to fill the content box starting at
    // `(dx, dy)`, aspect-preserved and centered horizontally — so a small guest
    // framebuffer (DOOM's 320×200) fills the window instead of sitting tiny in a
    // corner. `scale == 1` reproduces a plain 1:1 blit.
    let avail_w = (cw - dx).max(1);
    let avail_h = (ch - dy).max(1);
    let scale = (avail_w / w).min(avail_h / h).max(1);
    let x_off = dx + (avail_w - w * scale).max(0) / 2;
    let s = scale as usize;
    // Each source pixel becomes a scale×scale block. The image is scaled to fit
    // inside the content box, so the blocks never exceed it; `fill_rect` (which
    // clips to the framebuffer) draws each block on the fast 32-bit path.
    for row in 0..h {
        let py0 = (oy + dy + row * scale) as usize;
        for col in 0..w {
            let o = ((row * w + col) * 4) as usize;
            let color = Color::rgb(px[o], px[o + 1], px[o + 2]);
            c.fill_rect((ox + x_off + col * scale) as usize, py0, s, s, color);
        }
    }
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
    // host.blit(ptr, w, h, dx, dy): copy an RGBA frame from guest memory.
    linker
        .func_wrap(
            "host",
            "blit",
            |caller: Caller<'_, HostState>, off: i32, w: i32, h: i32, dx: i32, dy: i32| {
                host_blit(&caller, off, w, h, dx, dy);
            },
        )
        .map_err(|_| "link host.blit")?;
    // host.time_ms(): milliseconds since boot. The PIT ticks at 250 Hz, so each
    // tick is 4 ms. Lets a guest animate or time its own logic.
    linker
        .func_wrap("host", "time_ms", |_: Caller<'_, HostState>| -> i64 {
            (crate::interrupts::ticks() * 4) as i64
        })
        .map_err(|_| "link host.time_ms")?;
    // The WASI subset (wasi_snapshot_preview1) a C/clang guest like DOOM needs.
    // Harmless for guests that don't import it.
    wasi::install(linker)?;
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
    let module = match Module::new(&engine, APP_WASM) {
        Ok(m) => m,
        Err(e) => {
            crate::serial_println!("wasm app: decode failed: {:?}", e);
            return None;
        }
    };
    let mut store = Store::new(&engine, HostState::console());
    let mut linker = <Linker<HostState>>::new(&engine);
    install_abi(&mut linker).ok()?;
    let instance = match linker.instantiate_and_start(&mut store, &module) {
        Ok(i) => i,
        Err(e) => {
            crate::serial_println!("wasm app: instantiate failed: {:?}", e);
            return None;
        }
    };
    // A WASI "reactor" module (clang -mexec-model=reactor, e.g. DOOM) exposes
    // `_initialize`, which must run once to set up libc globals before any other
    // export is called. No-op for our hand-written/Rust modules.
    if let Ok(init) = instance.get_typed_func::<(), ()>(&store, "_initialize")
        && let Err(e) = init.call(&mut store, ())
    {
        crate::serial_println!("wasm app: _initialize trap: {:?}", e);
        return None;
    }
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

// ---- threaded app worker ----
//
// The resident app runs on its OWN kernel thread, not the compositor's: a heavy
// app (DOOM parsing a 4 MiB WAD, then running its game loop) must never block the
// UI. The worker renders into an offscreen surface; the compositor copies the
// latest finished frame into the window. Input is queued to the worker. This is
// the same decoupling the browser's background fetcher uses.

/// Offscreen surface size — the WASM window's content box (window 720×470).
const WASM_SW: usize = 692;
const WASM_SH: usize = 414;
const SURF_BYTES: usize = WASM_SW * WASM_SH * 4;

/// Double-buffered offscreen surfaces: the worker renders into the back buffer,
/// then publishes it as the front for the compositor — so no half-drawn frame
/// is ever shown.
static SURFACE: RacyCell<[[u8; SURF_BYTES]; 2]> = RacyCell::new([[0; SURF_BYTES]; 2]);
static FRONT: AtomicUsize = AtomicUsize::new(0);
static READY: AtomicBool = AtomicBool::new(false);
/// Set while the WASM window is open; the worker idles (`hlt`) otherwise.
static ACTIVE: AtomicBool = AtomicBool::new(false);
/// The real framebuffer layout, captured at boot to derive the offscreen one.
static FB_INFO: RacyCell<Option<FrameBufferInfo>> = RacyCell::new(None);

/// A queued input event for the worker. `kind`: 1 = key, 2 = pointer.
#[derive(Clone, Copy)]
struct Ev {
    kind: u8,
    a: i32,
    b: i32,
    c: i32,
}
const EV_CAP: usize = 128;
static EVENTS: RacyCell<[Ev; EV_CAP]> = RacyCell::new(
    [Ev {
        kind: 0,
        a: 0,
        b: 0,
        c: 0,
    }; EV_CAP],
);
static EV_HEAD: AtomicUsize = AtomicUsize::new(0); // producer (compositor/input)
static EV_TAIL: AtomicUsize = AtomicUsize::new(0); // consumer (worker)

/// Capture the framebuffer layout (call once at boot, before spawning [`worker`]).
pub fn init(info: FrameBufferInfo) {
    unsafe {
        *FB_INFO.get() = Some(info);
    }
}

/// Mark the WASM app window open/closed. The worker only runs while open.
pub fn set_active(on: bool) {
    ACTIVE.store(on, Ordering::Release);
}

fn push_ev(ev: Ev) {
    let h = EV_HEAD.load(Ordering::Relaxed);
    let next = (h + 1) % EV_CAP;
    if next == EV_TAIL.load(Ordering::Acquire) {
        return; // queue full — drop the event
    }
    unsafe {
        (*EVENTS.get())[h] = ev;
    }
    EV_HEAD.store(next, Ordering::Release);
}

/// Queue a pointer event (content-local coords; `buttons` bit 0 = left).
pub fn on_pointer(x: i32, y: i32, buttons: i32) {
    push_ev(Ev {
        kind: 2,
        a: x,
        b: y,
        c: buttons,
    });
}

/// Queue a key event (`code` is an ASCII byte, or 10 for Enter / 27 for Esc).
pub fn on_key(code: i32) {
    push_ev(Ev {
        kind: 1,
        a: code,
        b: 0,
        c: 0,
    });
}

/// Drain queued input into the app's `on_key` / `on_pointer` exports.
fn drain_input(app: &mut App) {
    loop {
        let t = EV_TAIL.load(Ordering::Relaxed);
        if t == EV_HEAD.load(Ordering::Acquire) {
            break;
        }
        let ev = unsafe { (*EVENTS.get())[t] };
        EV_TAIL.store((t + 1) % EV_CAP, Ordering::Release);
        match ev.kind {
            1 => {
                if let Ok(f) = app.instance.get_typed_func::<i32, ()>(&app.store, "on_key") {
                    let _ = f.call(&mut app.store, ev.a);
                }
            }
            2 => {
                if let Ok(f) =
                    app.instance
                        .get_typed_func::<(i32, i32, i32), ()>(&app.store, "on_pointer")
                {
                    let _ = f.call(&mut app.store, (ev.a, ev.b, ev.c));
                }
            }
            _ => {}
        }
    }
}

/// The offscreen framebuffer layout: content-box sized, same pixel format as the
/// real screen so the compositor can copy rows verbatim.
fn surface_info() -> Option<FrameBufferInfo> {
    let mut oi = unsafe { *FB_INFO.get() }?;
    oi.width = WASM_SW;
    oi.height = WASM_SH;
    oi.stride = WASM_SW;
    Some(oi)
}

/// Worker-thread entry: build the app, then loop — drain input, render one frame
/// into the back surface, publish it. Idles (`hlt`) while the window is closed or
/// the app has not loaded yet.
pub extern "C" fn worker() -> ! {
    loop {
        if !ACTIVE.load(Ordering::Acquire) {
            x86_64::instructions::hlt();
            continue;
        }
        let (Some(app), Some(oi)) = (app_mut(), surface_info()) else {
            x86_64::instructions::hlt();
            continue;
        };
        let back = 1 - FRONT.load(Ordering::Relaxed);
        {
            let buf = unsafe { &mut (*SURFACE.get())[back] };
            let st = app.store.data_mut();
            st.fb = buf.as_mut_ptr();
            st.fb_len = buf.len();
            st.info = Some(oi);
            st.ox = 0;
            st.oy = 0;
            st.cw = WASM_SW as i32;
            st.ch = WASM_SH as i32;
        }
        drain_input(app);
        if let Ok(func) = app.instance.get_typed_func::<(), ()>(&app.store, "render")
            && let Err(e) = func.call(&mut app.store, ())
        {
            static LOGGED: AtomicBool = AtomicBool::new(false);
            if !LOGGED.swap(true, Ordering::Relaxed) {
                crate::serial_println!("wasm app: render trap: {:?}", e);
            }
        }
        app.store.data_mut().info = None;
        FRONT.store(back, Ordering::Release);
        READY.store(true, Ordering::Release);
    }
}

/// Copy the latest finished app frame into the live `Canvas` at content origin
/// `(cx, cy)`. Shows a placeholder until the first frame is ready (the worker may
/// still be loading — e.g. DOOM parsing its WAD).
pub fn blit_surface(c: &mut Canvas, cx: i32, cy: i32) {
    if !READY.load(Ordering::Acquire) {
        c.fill_rect(
            cx.max(0) as usize,
            cy.max(0) as usize,
            WASM_SW,
            WASM_SH,
            Color::rgb(0x10, 0x14, 0x20),
        );
        font::draw_text(
            c,
            (cx + 16).max(0) as usize,
            (cy + 16).max(0) as usize,
            "Carregando app WASM...",
            Color::rgb(0x9a, 0xa6, 0xbd),
            2,
        );
        return;
    }
    let info = c.fb_info();
    let bpp = info.bytes_per_pixel;
    let stride = info.stride;
    let (sw, sh) = (info.width as i32, info.height as i32);
    let front = FRONT.load(Ordering::Acquire);
    let src = unsafe { &(*SURFACE.get())[front] };
    let dst = c.buffer_mut();
    for y in 0..WASM_SH as i32 {
        let dyy = cy + y;
        if dyy < 0 || dyy >= sh {
            continue;
        }
        let x0 = cx.max(0);
        let x1 = (cx + WASM_SW as i32).min(sw);
        if x1 <= x0 {
            continue;
        }
        let cols = (x1 - x0) as usize;
        let so = (y as usize * WASM_SW + (x0 - cx) as usize) * bpp;
        let dofs = (dyy as usize * stride + x0 as usize) * bpp;
        dst[dofs..dofs + cols * bpp].copy_from_slice(&src[so..so + cols * bpp]);
    }
}

/// The guest's exported linear memory. WAT modules here export it as `mem`;
/// Rust/clang-compiled modules export it as `memory` — accept either.
fn guest_mem(caller: &Caller<'_, HostState>) -> Option<wasmi::Memory> {
    for name in ["memory", "mem"] {
        if let Some(Extern::Memory(m)) = caller.get_export(name) {
            return Some(m);
        }
    }
    None
}

/// Borrow `len` raw bytes at offset `ptr` from the guest's linear memory.
/// `None` on missing memory or an out-of-bounds range.
fn guest_bytes<'a>(caller: &'a Caller<'_, HostState>, ptr: i32, len: i32) -> Option<&'a [u8]> {
    let mem = guest_mem(caller)?;
    let data = mem.data(caller);
    let (ptr, len) = (ptr as usize, len.max(0) as usize);
    data.get(ptr..ptr.saturating_add(len))
}

/// Borrow a UTF-8 string of `len` bytes at offset `ptr` from guest memory.
/// `None` on missing memory, out-of-bounds range, or non-UTF-8.
fn guest_str<'a>(caller: &'a Caller<'_, HostState>, ptr: i32, len: i32) -> Option<&'a str> {
    core::str::from_utf8(guest_bytes(caller, ptr, len)?).ok()
}
