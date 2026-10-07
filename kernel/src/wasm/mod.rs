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
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use wasmi::{
    Caller, Config, Engine, Extern, Instance, Linker, Module, Store, StoreLimits,
    StoreLimitsBuilder,
};

// ---- guest resource limits ----
//
// A guest is untrusted code running inside the kernel: without limits a loop or
// a runaway `memory.grow` takes the whole machine down with it. Every guest gets
//   * fuel (one unit per wasm instruction) per call into it,
//   * a hard cap on its linear memory, and
//   * caps on the work the host does on its behalf (see `wasi.rs`, `host_blit`).

/// Fuel for one call into the guest (`render`, `on_key`, `on_pointer`). About
/// 20 million wasm instructions: DOOM's steady-state frame is 4.5-9 M (see
/// docs/audit/adr-isolamento.md), so this leaves ~2x headroom, yet a guest stuck
/// in a loop is stopped within a few frames' worth of CPU.
const FRAME_FUEL: u64 = 20_000_000;
/// Fuel for module start-up: `_initialize` and the first `render`, where C apps do
/// their one-time setup (DOOM's `doomgeneric_Create` costs ~39 M). About 6x that.
const INIT_FUEL: u64 = 256_000_000;
/// Cap on a guest's linear memory, in bytes. DOOM peaks at ~16-20 MiB; 24 MiB
/// keeps ~20% headroom while bounding a hostile `memory.grow` to well under half
/// of the kernel's shared 64 MiB heap (without a cap it reached 32 MiB, 51%).
const MEM_LIMIT: usize = 24 << 20;
/// Cap on a guest's table size (DOOM's indirect-call table has a few thousand).
const TABLE_LIMIT: usize = 100_000;
/// Longest string (bytes) a guest may pass to `host.log` / `host.draw_text`.
const MAX_TEXT: i32 = 4096;
/// Largest image (pixels) one `host.blit` will copy: more than any window here.
const MAX_BLIT_PIXELS: i64 = 1 << 20;

/// The limits applied to every guest `Store`.
fn guest_limits() -> StoreLimits {
    StoreLimitsBuilder::new()
        .memory_size(MEM_LIMIT)
        .table_elements(TABLE_LIMIT)
        .instances(1)
        .memories(1)
        .tables(1)
        .build()
}

/// An engine with fuel metering switched on (off by default in wasmi).
fn guest_engine() -> Engine {
    let mut cfg = Config::default();
    cfg.consume_fuel(true);
    Engine::new(&cfg)
}

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
/// functions from inside `wasmi`, outliving any normal borrow. The `HostState` of
/// the resident app lives in its `Store`, which only the `wasmapp` worker thread
/// touches; the console demo runs once on the boot thread before the scheduler
/// starts. Either way one thread at a time owns it.
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
    /// Memory/table/instance caps, enforced by the `Store`'s resource limiter.
    limits: StoreLimits,
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
            limits: guest_limits(),
        }
    }

    /// Build a `Canvas` over the surface, or `None` for console-only guests.
    fn canvas(&self) -> Option<Canvas<'static>> {
        let info = self.info?;
        // SAFETY: only the thread that owns this `Store` (the `wasmapp` worker, or the boot thread for
        // the console demo) calls this; the pointer/length come from a live surface slice that outlives
        // this guest call (set right before it, cleared right after).
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
///
/// The copy is host work the fuel meter cannot see, so it is capped
/// ([`MAX_BLIT_PIXELS`]) and charged to the guest's fuel (one unit per 8 pixels).
fn host_blit(
    caller: &mut Caller<'_, HostState>,
    off: i32,
    w: i32,
    h: i32,
    dx: i32,
    dy: i32,
) -> Result<(), wasmi::Error> {
    if w <= 0 || h <= 0 || (w as i64) * (h as i64) > MAX_BLIT_PIXELS {
        return Ok(());
    }
    charge(caller, (w as u64) * (h as u64) / 8)?;
    let caller = &*caller;
    let st = caller.data();
    let (ox, oy, cw, ch) = (st.ox, st.oy, st.cw, st.ch);
    let Some(mut c) = st.canvas() else {
        return Ok(());
    };
    let need = (w as i64) * (h as i64) * 4;
    let Some(px) = guest_bytes(caller, off, need.min(i32::MAX as i64) as i32) else {
        return Ok(());
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
    Ok(())
}

/// Charge `units` of fuel for host work done on the guest's behalf. Fails with
/// the `OutOfFuel` trap when the guest's budget for this call is spent.
fn charge(caller: &mut Caller<'_, HostState>, units: u64) -> Result<(), wasmi::Error> {
    let left = caller.get_fuel()?;
    if left < units {
        return Err(wasmi::Error::from(wasmi::TrapCode::OutOfFuel));
    }
    caller.set_fuel(left - units)
}

/// Register the OS ABI on `linker`. Shared by one-shot runs and the persistent
/// windowed app so every guest sees the same host surface.
fn install_abi(linker: &mut Linker<HostState>) -> Result<(), &'static str> {
    linker
        .func_wrap(
            "host",
            "log",
            |caller: Caller<'_, HostState>, ptr: i32, len: i32| {
                if len > MAX_TEXT {
                    return;
                }
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
                if len > MAX_TEXT {
                    return;
                }
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
            |mut caller: Caller<'_, HostState>,
             off: i32,
             w: i32,
             h: i32,
             dx: i32,
             dy: i32|
             -> Result<(), wasmi::Error> { host_blit(&mut caller, off, w, h, dx, dy) },
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
    let engine = guest_engine();
    let module = Module::new(&engine, bytes).map_err(|_| "module decode")?;
    let mut store = Store::new(&engine, state);
    store.limiter(|st| &mut st.limits);
    store.set_fuel(INIT_FUEL).map_err(|_| "set fuel")?;
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
    /// The next `render` is the first one: it gets [`INIT_FUEL`] (one-time setup).
    first_render: bool,
}

/// Why a guest call failed (for the serial log) and, by returning at all, that the
/// app must be terminated.
fn describe(e: &wasmi::Error) -> &'static str {
    if e.as_trap_code() == Some(wasmi::TrapCode::OutOfFuel) {
        "out of fuel"
    } else if e.i32_exit_status().is_some() {
        "exited (proc_exit)"
    } else {
        "trapped"
    }
}

/// Call the guest export `name(params)` with a fresh fuel budget. A missing
/// export is not an error (apps implement only the hooks they need).
fn guest_call<P: wasmi::WasmParams>(
    app: &mut App,
    name: &str,
    params: P,
    fuel: u64,
) -> Result<(), wasmi::Error> {
    let Ok(f) = app.instance.get_typed_func::<P, ()>(&app.store, name) else {
        return Ok(());
    };
    app.store.set_fuel(fuel)?;
    f.call(&mut app.store, params)
}

/// The desktop's WASM app, built lazily on first paint and kept resident so the
/// interpreter is not re-instantiated every frame. Only the `wasmapp` worker
/// thread touches it (build, run, drop on close); the compositor talks to that
/// thread through `ACTIVE`, the event queue and the surface buffers instead.
static APP: RacyCell<Option<App>> = RacyCell::new(None);

/// Build the resident windowed app from `APP_WASM`. `None` if it fails to load.
fn build_app() -> Option<App> {
    let engine = guest_engine();
    let module = match Module::new(&engine, APP_WASM) {
        Ok(m) => m,
        Err(e) => {
            crate::serial_println!("wasm app: decode failed: {:?}", e);
            return None;
        }
    };
    let mut store = Store::new(&engine, HostState::console());
    store.limiter(|st| &mut st.limits);
    store.set_fuel(INIT_FUEL).ok()?;
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
        crate::serial_println!("wasm app: _initialize {}: {:?}", describe(&e), e);
        return None;
    }
    Some(App {
        store,
        instance,
        first_render: true,
    })
}

/// Terminate the resident app: drop its `Store` (frees the guest's linear memory
/// back to the kernel heap) and forget its queued input. Worker thread only.
fn drop_app() {
    // SAFETY: APP is only touched by the `wasmapp` worker thread (see `app_mut`), and no reference
    // obtained from `app_mut` is live here (the worker's loop iteration is over).
    unsafe { *APP.get() = None };
    EV_TAIL.store(EV_HEAD.load(Ordering::Acquire), Ordering::Release);
    READY.store(false, Ordering::Release);
}

/// The resident app, built lazily on first access. `None` if it fails to load.
/// Callable only from the `wasmapp` worker thread (see [`APP`]).
fn app_mut() -> Option<&'static mut App> {
    // SAFETY: APP is only reached through `app_mut`, called only by the `wasmapp` worker thread (the
    // compositor never uses it), and no reference outlives a loop iteration, so the `&mut` is unique.
    // NOTE: not guaranteed by the type (safe fn returning `&'static mut`).
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

/// Minimum spacing between two `render` calls of the resident app, in timer
/// ticks (4 ms each): 4 ticks = 16 ms, about 62 frames per second.
const RENDER_PERIOD_TICKS: u64 = 4;

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
/// Set while the WASM window is open; the worker is blocked in the scheduler otherwise.
static ACTIVE: AtomicBool = AtomicBool::new(false);
/// Scheduler slot of the worker thread (`usize::MAX` until it has started), so the
/// compositor can wake it on window open and on input.
static TID: AtomicUsize = AtomicUsize::new(usize::MAX);

/// Wake the worker if it is blocked (no-op before it has started).
fn wake_worker() {
    let tid = TID.load(Ordering::Acquire);
    if tid != usize::MAX {
        crate::sched::wake(tid);
    }
}
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
    // SAFETY: called once, before `wasmapp` is spawned (`kernel_main`), so the worker cannot read
    // FB_INFO yet.
    unsafe {
        *FB_INFO.get() = Some(info);
    }
}

/// Mark the WASM app window open/closed. The worker only runs while open.
pub fn set_active(on: bool) {
    if ACTIVE.swap(on, Ordering::AcqRel) != on {
        wake_worker();
    }
}

fn push_ev(ev: Ev) {
    let h = EV_HEAD.load(Ordering::Relaxed);
    let next = (h + 1) % EV_CAP;
    if next == EV_TAIL.load(Ordering::Acquire) {
        return; // queue full — drop the event
    }
    // SAFETY: SPSC queue and this is its only producer (compositor input path). Slot `h` is not
    // readable by the consumer until EV_HEAD is published below, and `next != EV_TAIL` keeps it
    // off the slot being read.
    unsafe {
        (*EVENTS.get())[h] = ev;
    }
    EV_HEAD.store(next, Ordering::Release);
    wake_worker();
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

/// Drain queued input into the app's `on_key` / `on_pointer` exports, each call
/// with its own fuel budget. The first guest failure aborts the drain.
fn drain_input(app: &mut App) -> Result<(), wasmi::Error> {
    loop {
        let t = EV_TAIL.load(Ordering::Relaxed);
        if t == EV_HEAD.load(Ordering::Acquire) {
            return Ok(());
        }
        // SAFETY: SPSC consumer (worker only): `t != EV_HEAD` (Acquire) means the producer already wrote
        // slot `t` and will not reuse it until EV_TAIL advances.
        let ev = unsafe { (*EVENTS.get())[t] };
        EV_TAIL.store((t + 1) % EV_CAP, Ordering::Release);
        match ev.kind {
            1 => guest_call(app, "on_key", ev.a, FRAME_FUEL)?,
            2 => guest_call(app, "on_pointer", (ev.a, ev.b, ev.c), FRAME_FUEL)?,
            _ => {}
        }
    }
}

/// The offscreen framebuffer layout: content-box sized, same pixel format as the
/// real screen so the compositor can copy rows verbatim.
fn surface_info() -> Option<FrameBufferInfo> {
    // SAFETY: FB_INFO is written once in `init`, before this thread is spawned, and only read after.
    let mut oi = unsafe { *FB_INFO.get() }?;
    oi.width = WASM_SW;
    oi.height = WASM_SH;
    oi.stride = WASM_SW;
    Some(oi)
}

/// What the window shows instead of a frame (see [`blit_surface`]): still loading.
const MSG_LOADING: u8 = 0;
/// The app could not be loaded (decode/instantiate/`_initialize` failed).
const MSG_LOAD_FAILED: u8 = 1;
/// The app was terminated (trap, out of fuel, `proc_exit`).
const MSG_TERMINATED: u8 = 2;
static MSG: AtomicU8 = AtomicU8::new(MSG_LOADING);

/// Kill the resident app: log why, free its `Store`, and leave the window showing
/// `msg` until it is closed (closing and reopening starts a fresh instance).
fn terminate(msg: u8, what: &str, e: Option<&wasmi::Error>) {
    match e {
        Some(e) => crate::serial_println!("wasm app: terminated: {} ({:?})", what, e),
        None => crate::serial_println!("wasm app: terminated: {}", what),
    }
    drop_app();
    MSG.store(msg, Ordering::Release);
}

/// Worker-thread entry: build the app, then loop — drain input, render one frame
/// into the back surface, publish it. While the window is closed, or the app has
/// been terminated, the thread is blocked in the scheduler and costs no CPU.
/// Closing the window drops the instance (and its memory); reopening rebuilds it.
pub extern "C" fn worker() -> ! {
    TID.store(crate::sched::current(), Ordering::Release);
    loop {
        if !ACTIVE.load(Ordering::Acquire) {
            // Closed: end the app for real, then sleep until `set_active(true)`.
            drop_app();
            MSG.store(MSG_LOADING, Ordering::Release);
            crate::sched::block(crate::sched::FOREVER, || !ACTIVE.load(Ordering::Acquire));
            continue;
        }
        if MSG.load(Ordering::Acquire) != MSG_LOADING {
            // Terminated or failed to load: nothing to run until the window is
            // closed (`set_active(false)` wakes us) and reopened.
            crate::sched::block(crate::sched::FOREVER, || ACTIVE.load(Ordering::Acquire));
            continue;
        }
        let (Some(app), Some(oi)) = (app_mut(), surface_info()) else {
            terminate(MSG_LOAD_FAILED, "failed to load", None);
            continue;
        };
        let frame_start = crate::interrupts::ticks();
        let back = 1 - FRONT.load(Ordering::Relaxed);
        {
            // SAFETY: the worker is the only writer of SURFACE[back] (the buffer not published as FRONT); the
            // raw pointer kept in HostState is disabled (`info = None`) once the frame is done.
            // NOTE: not prevented: if the compositor is preempted inside `blit_surface` while the worker flips
            // FRONT, the worker rewrites the buffer being copied (torn frame; formally a data race).
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
        if let Err(e) = drain_input(app) {
            terminate(MSG_TERMINATED, describe(&e), Some(&e));
            continue;
        }
        let fuel = if core::mem::take(&mut app.first_render) {
            INIT_FUEL
        } else {
            FRAME_FUEL
        };
        if let Err(e) = guest_call(app, "render", (), fuel) {
            terminate(MSG_TERMINATED, describe(&e), Some(&e));
            continue;
        }
        app.store.data_mut().info = None;
        FRONT.store(back, Ordering::Release);
        READY.store(true, Ordering::Release);

        // Pace the app: at most one `render` per `RENDER_PERIOD_TICKS`. The
        // compositor shows a frame at most every few ms anyway, so rendering
        // faster only steals CPU from it; apps keep their own time via
        // `host.time_ms`. Input cuts the nap short (`push_ev` wakes us).
        crate::sched::block(frame_start + RENDER_PERIOD_TICKS, || {
            ACTIVE.load(Ordering::Acquire)
                && EV_TAIL.load(Ordering::Acquire) == EV_HEAD.load(Ordering::Acquire)
        });
    }
}

/// True if the worker thread died (panic or CPU fault inside the app engine, see
/// `sched::kill_current`). The window then shows the same "encerrado" notice as a
/// normal termination; the thread is not restarted.
fn worker_dead() -> bool {
    let tid = TID.load(Ordering::Acquire);
    tid != usize::MAX && crate::sched::is_dead(tid)
}

/// Copy the latest finished app frame into the live `Canvas` at content origin
/// `(cx, cy)`. Shows a placeholder until the first frame is ready (the worker may
/// still be loading — e.g. DOOM parsing its WAD).
pub fn blit_surface(c: &mut Canvas, cx: i32, cy: i32) {
    let dead = worker_dead();
    if dead || !READY.load(Ordering::Acquire) {
        c.fill_rect(
            cx.max(0) as usize,
            cy.max(0) as usize,
            WASM_SW,
            WASM_SH,
            Color::rgb(0x10, 0x14, 0x20),
        );
        let text = match MSG.load(Ordering::Acquire) {
            _ if dead => "App WASM encerrado",
            MSG_LOAD_FAILED => "App WASM nao carregou",
            MSG_TERMINATED => "App WASM encerrado",
            _ => "Carregando app WASM...",
        };
        font::draw_text(
            c,
            (cx + 16).max(0) as usize,
            (cy + 16).max(0) as usize,
            text,
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
    // SAFETY: shared read of the published `front` buffer; the worker writes the other one.
    // NOTE: the roles can swap while we copy (see `worker`): torn frame, not excluded by the type.
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
