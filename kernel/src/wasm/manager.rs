//! `AppManager`: many WebAssembly apps at once.
//!
//! # Threads and ownership
//!
//! * The **compositor thread** (desktop) opens, resizes, feeds input to, draws and
//!   closes apps through the functions at the bottom of this file. It only touches
//!   the [`Slot`] table, never a wasm `Store`.
//! * The **`appd` thread** ([`worker`]) is the only owner of the wasm runtimes
//!   (`RT`): it instantiates, runs and drops every `Store`. One thread, not one per
//!   app, because the kernel has 8 thread slots and a single owner needs no locks
//!   between apps.
//! * The `Slot` table is shared; the kernel is single-core, so every access runs with
//!   interrupts masked ([`with`]) and is short. Surfaces are the exception: they are
//!   copied outside the lock under the `reading` handshake (see [`blit`]).
//!
//! # Scheduling
//!
//! `appd` loops over the slots round-robin. A slot is *ready* when it has input, a
//! redraw request, a size change, a tick due, or (legacy v1 apps) a frame due. Each
//! ready app runs **one slice** per pass: at most [`SLICE_EVENTS`] event calls and one
//! `render`, each call with its own fuel budget (`fuel_frame` from the manifest). No
//! app runs two slices before every other ready app ran one. With nothing ready,
//! `appd` blocks in `sched::block` until the nearest deadline or a `wake` (input,
//! open, close): an idle platform costs no CPU.
//!
//! # Failure
//!
//! Any failed guest call (out of fuel, trap, invalid pointer, `exit`) ends **that
//! app only**: its runtime (and with it its memory and open files) is dropped on the
//! spot and the slot keeps the reason, which the window shows.

use super::{
    AppFault, FRAME_FUEL, HostState, INIT_FUEL, abi2, describe, guest_engine, guest_limits,
    install_all,
};
use crate::sync::RacyCell;
use crate::{io, serial_println};
use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use bootloader_api::info::FrameBufferInfo;
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use osjeff_core::appfs::Sandbox;
use osjeff_core::appmanifest::{Manifest, Quotas};
use wasmi::{Instance, Linker, Memory, Module, Store, Val};

/// Handle of a running app (never reused).
pub type AppId = u32;

/// Most apps alive at once.
pub const MAX_APPS: usize = 8;
const EV_CAP: usize = 64;
/// Event calls per slice.
const SLICE_EVENTS: usize = 8;
/// Legacy v1 apps render every 4 ticks (16 ms).
const FRAME_TICKS: u64 = 4;
/// Window of the CPU percentage (1 s at 250 Hz).
const CPU_WINDOW_TICKS: u64 = 250;
/// A resize is applied once the requested size has been stable this many ticks.
const RESIZE_SETTLE_TICKS: u64 = 3;
/// Most pixels one surface may have (the biggest window is 1280x800 content).
const MAX_SURFACE_PIXELS: usize = 1280 * 800;

/// Life-cycle state of an app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Starting,
    Running,
    /// Window minimized: no render, no ticks, input dropped.
    Suspended,
    /// Left by itself (`exit` / `proc_exit`).
    Exited,
    Crashed,
}

impl State {
    /// Three-letter label for the Task Manager.
    pub fn label(self) -> &'static str {
        match self {
            State::Starting => "INI",
            State::Running => "RUN",
            State::Suspended => "SUS",
            State::Exited => "END",
            State::Crashed => "ERR",
        }
    }
}

#[derive(Clone, Copy)]
enum Ev {
    Key(i32, i32),
    Text(i32),
    Pointer(i32, i32, i32),
    Close,
}

struct Slot {
    id: AppId,
    manifest: Manifest,
    quotas: Quotas,
    v1: bool,
    wasm: Arc<[u8]>,
    state: State,
    reason: String,
    events: VecDeque<Ev>,
    want_redraw: bool,
    visible: bool,
    /// Content size the window wants, and when that changed.
    req_w: i32,
    req_h: i32,
    req_at: u64,
    /// Content size of the surfaces (and of what the guest knows).
    cur_w: i32,
    cur_h: i32,
    title: String,
    surf: [Vec<u8>; 2],
    front: usize,
    ready: bool,
    /// Surface index the compositor is copying right now.
    reading: Option<usize>,
    closing: bool,
    restart: bool,
    rt_dropped: bool,
    // ---- accounting ----
    cpu_total: u64,
    cpu_win: u64,
    cpu_win_start: u64,
    cpu_pct: u8,
    fuel_total: u64,
    mem_kib: u32,
    mem_peak_kib: u32,
    calls: u64,
    frames: u64,
    started_tick: u64,
    last_frame: u64,
    last_tick: u64,
}

const NONE_SLOT: Option<Slot> = None;
static SLOTS: RacyCell<[Option<Slot>; MAX_APPS]> = RacyCell::new([NONE_SLOT; MAX_APPS]);
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Run `f` with exclusive access to the slot table (single core, IRQs masked).
fn with<R>(f: impl FnOnce(&mut [Option<Slot>; MAX_APPS]) -> R) -> R {
    x86_64::instructions::interrupts::without_interrupts(|| {
        // SAFETY: single core and interrupts masked for the closure, which never re-enters `with`,
        // so this is the only live reference to the table.
        f(unsafe { &mut *SLOTS.get() })
    })
}

fn slot_of(slots: &mut [Option<Slot>; MAX_APPS], id: AppId) -> Option<&mut Slot> {
    slots.iter_mut().flatten().find(|s| s.id == id)
}

// ---------------------------------------------------------------- runtimes (appd only)

struct Runtime {
    store: Store<HostState>,
    instance: Instance,
    memory: Option<Memory>,
    first_render: bool,
    /// Fuel consumed and calls made since the last `account`.
    fuel_spent: u64,
    calls: u64,
    /// Button state of the last pointer event delivered (v1 apps see only presses).
    last_buttons: i32,
    /// The guest asked for a redraw while rendering.
    redraw_again: bool,
}

static RT: RacyCell<[Option<Box<Runtime>>; MAX_APPS]> = RacyCell::new([const { None }; MAX_APPS]);

fn rts() -> &'static mut [Option<Box<Runtime>>; MAX_APPS] {
    // SAFETY: RT is only touched by the `appd` worker thread (build, run, drop); no reference outlives
    // one loop iteration there.
    // NOTE: not guaranteed by the type (safe fn returning `&'static mut`).
    unsafe { &mut *RT.get() }
}

static TID: AtomicUsize = AtomicUsize::new(usize::MAX);
static TSC_KHZ: AtomicU64 = AtomicU64::new(1_000_000);
/// The real framebuffer layout, captured at boot to derive the offscreen one.
static FB_INFO: RacyCell<Option<FrameBufferInfo>> = RacyCell::new(None);

/// Capture the framebuffer layout and TSC rate (once, before `appd` spawns).
pub fn init(info: FrameBufferInfo, tsc_khz: u64) {
    // SAFETY: called once, before `appd` is spawned (`kernel_main`), so nothing can read FB_INFO yet.
    unsafe {
        *FB_INFO.get() = Some(info);
    }
    TSC_KHZ.store(tsc_khz.max(1), Ordering::Relaxed);
}

fn fb_info() -> Option<FrameBufferInfo> {
    // SAFETY: written once in `init`, before the readers exist; only read afterwards.
    unsafe { *FB_INFO.get() }
}

fn surface_info(w: i32, h: i32) -> Option<FrameBufferInfo> {
    let mut oi = fb_info()?;
    oi.width = w.max(1) as usize;
    oi.height = h.max(1) as usize;
    oi.stride = w.max(1) as usize;
    oi.byte_len = oi.stride * oi.height * oi.bytes_per_pixel;
    Some(oi)
}

fn wake_worker() {
    let tid = TID.load(Ordering::Acquire);
    if tid != usize::MAX {
        crate::sched::wake(tid);
    }
}

// ---------------------------------------------------------------- launch / close (compositor)

/// Why an app could not be started.
#[derive(Debug, PartialEq, Eq)]
pub enum LaunchError {
    /// All [`MAX_APPS`] slots are in use.
    Full,
}

/// Start an app from its package bytes. `w`/`h` are the window's content size.
/// The module is instantiated by `appd`; failures show up as `Crashed` in the window.
pub fn launch(
    wasm: Vec<u8>,
    manifest: Manifest,
    v1: bool,
    w: i32,
    h: i32,
) -> Result<AppId, LaunchError> {
    let quotas = manifest.granted();
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed) as AppId;
    let now = crate::interrupts::ticks();
    let (w, h) = (w.clamp(1, 1280), h.clamp(1, 800));
    let slot = Slot {
        id,
        manifest,
        quotas,
        v1,
        wasm: Arc::from(wasm),
        state: State::Starting,
        reason: String::new(),
        events: VecDeque::new(),
        want_redraw: true,
        visible: true,
        req_w: w,
        req_h: h,
        req_at: now,
        cur_w: 0,
        cur_h: 0,
        title: String::new(),
        surf: [Vec::new(), Vec::new()],
        front: 0,
        ready: false,
        reading: None,
        closing: false,
        restart: false,
        rt_dropped: true,
        cpu_total: 0,
        cpu_win: 0,
        cpu_win_start: now,
        cpu_pct: 0,
        fuel_total: 0,
        mem_kib: 0,
        mem_peak_kib: 0,
        calls: 0,
        frames: 0,
        started_tick: now,
        last_frame: 0,
        last_tick: now,
    };
    let ok = with(|slots| match slots.iter_mut().find(|s| s.is_none()) {
        Some(free) => {
            *free = Some(slot);
            true
        }
        None => false,
    });
    if !ok {
        return Err(LaunchError::Full);
    }
    wake_worker();
    Ok(id)
}

/// The window was asked to close: tell the app (`on_close`) and mark it for removal.
pub fn request_close(id: AppId) {
    with(|slots| {
        if let Some(s) = slot_of(slots, id)
            && matches!(s.state, State::Running | State::Suspended)
        {
            s.events.push_back(Ev::Close);
        }
    });
    wake_worker();
}

/// The window is gone: drop the app (runtime first, by `appd`; the slot on `reap`).
pub fn close(id: AppId) {
    with(|slots| {
        if let Some(s) = slot_of(slots, id) {
            s.closing = true;
            s.events.clear();
        }
    });
    wake_worker();
}

/// Restart an app: new `Store` from the same package (Task Manager, `R`).
pub fn restart(id: AppId) {
    with(|slots| {
        if let Some(s) = slot_of(slots, id)
            && !s.closing
        {
            s.restart = true;
        }
    });
    wake_worker();
}

/// Free the slots of apps whose runtime `appd` has dropped. Call every frame.
pub fn reap() {
    with(|slots| {
        for s in slots.iter_mut() {
            let done = matches!(s, Some(x) if x.closing && x.rt_dropped && x.reading.is_none());
            if done {
                *s = None; // surfaces are freed here, on the compositor thread
            }
        }
    });
}

/// Window geometry and visibility, once per frame: the content size the surface
/// should have, and whether the window is on screen (minimized -> suspended).
/// Returns the app's title if the guest set one.
pub fn sync_window(id: AppId, w: i32, h: i32, visible: bool) -> Option<String> {
    let now = crate::interrupts::ticks();
    let mut wake = false;
    let title = with(|slots| {
        let s = slot_of(slots, id)?;
        let (w, h) = (w.clamp(1, 1280), h.clamp(1, 800));
        if (w, h) != (s.req_w, s.req_h) {
            s.req_w = w;
            s.req_h = h;
            s.req_at = now;
            wake = true;
        }
        if visible != s.visible {
            s.visible = visible;
            match (visible, s.state) {
                (false, State::Running) => s.state = State::Suspended,
                (true, State::Suspended) => {
                    s.state = State::Running;
                    s.want_redraw = true;
                    wake = true;
                }
                _ => {}
            }
        }
        (!s.title.is_empty()).then(|| s.title.clone())
    });
    if wake {
        wake_worker();
    }
    title
}

// ---------------------------------------------------------------- input (compositor)

fn push_ev(id: AppId, ev: Ev) {
    let queued = with(|slots| {
        let Some(s) = slot_of(slots, id) else {
            return false;
        };
        if !matches!(s.state, State::Running | State::Starting) || s.closing {
            return false;
        }
        // Coalesce pointer motion: only the newest position matters.
        if let (Ev::Pointer(_, _, b), Some(Ev::Pointer(_, _, lb))) = (ev, s.events.back())
            && b == *lb
        {
            s.events.pop_back();
        }
        if s.events.len() >= EV_CAP {
            return false;
        }
        s.events.push_back(ev);
        true
    });
    if queued {
        wake_worker();
    }
}

/// Key press: `code` per the ABI (ASCII, 10 Enter, 27 Esc, 8 BS, 127 Del, 0x100.. arrows),
/// `mods` bit0 Shift, bit1 Ctrl, bit2 Alt.
pub fn key(id: AppId, code: i32, mods: i32) {
    push_ev(id, Ev::Key(code, mods));
    // Printable keys also arrive as text (already translated by the keymap).
    if (0x20..0x7F).contains(&code) && mods & 6 == 0 {
        push_ev(id, Ev::Text(code));
    }
}

/// Pointer position (content-local) and buttons (bit0 left, bit1 right).
pub fn pointer(id: AppId, x: i32, y: i32, buttons: i32) {
    push_ev(id, Ev::Pointer(x, y, buttons));
}

// ---------------------------------------------------------------- clipboard (shared with the desktop)

static CLIP: RacyCell<([u8; 256], usize)> = RacyCell::new(([0; 256], 0));
static CLIP_GEN: AtomicU64 = AtomicU64::new(0);

/// Copy the shared clipboard into `out`; returns the length.
pub fn clip_get(out: &mut [u8]) -> usize {
    x86_64::instructions::interrupts::without_interrupts(|| {
        // SAFETY: single core with interrupts masked for the closure.
        let (buf, len) = unsafe { &*CLIP.get() };
        let n = (*len).min(out.len());
        out[..n].copy_from_slice(&buf[..n]);
        n
    })
}

/// Load the desktop's clipboard into the shared buffer (no generation bump).
pub fn clip_load(data: &[u8]) {
    x86_64::instructions::interrupts::without_interrupts(|| {
        // SAFETY: single core with interrupts masked for the closure.
        let (buf, len) = unsafe { &mut *CLIP.get() };
        let n = data.len().min(buf.len());
        buf[..n].copy_from_slice(&data[..n]);
        *len = n;
    });
}

/// Replace the shared clipboard (app side) and bump its generation.
pub fn clip_set(data: &[u8]) {
    x86_64::instructions::interrupts::without_interrupts(|| {
        // SAFETY: single core with interrupts masked for the closure.
        let (buf, len) = unsafe { &mut *CLIP.get() };
        let n = data.len().min(buf.len());
        buf[..n].copy_from_slice(&data[..n]);
        *len = n;
    });
    CLIP_GEN.fetch_add(1, Ordering::Release);
}

/// Generation counter of app-side clipboard writes (the desktop mirrors changes).
pub fn clip_generation() -> u64 {
    CLIP_GEN.load(Ordering::Acquire)
}

// ---------------------------------------------------------------- status (Task Manager, desktop)

/// A snapshot of one app for the Task Manager.
pub struct Status {
    pub id: AppId,
    pub app_id: String,
    pub state: State,
    pub cpu_pct: u8,
    pub mem_kib: u32,
}

/// Roll the CPU window if a second has passed.
fn roll_cpu(s: &mut Slot, now: u64) {
    let elapsed = now.saturating_sub(s.cpu_win_start);
    if elapsed >= CPU_WINDOW_TICKS {
        let khz = TSC_KHZ.load(Ordering::Relaxed).max(1);
        // cycles available in `elapsed` ticks (4 ms each)
        let avail = khz.saturating_mul(4).saturating_mul(elapsed).max(1);
        s.cpu_pct = (s.cpu_win.saturating_mul(100) / avail).min(100) as u8;
        s.cpu_win = 0;
        s.cpu_win_start = now;
    }
}

/// Status of every live app, in slot order.
pub fn statuses() -> Vec<Status> {
    let now = crate::interrupts::ticks();
    with(|slots| {
        slots
            .iter_mut()
            .flatten()
            .filter(|s| !s.closing)
            .map(|s| {
                roll_cpu(s, now);
                if now.saturating_sub(s.cpu_win_start) > 2 * CPU_WINDOW_TICKS {
                    s.cpu_pct = 0;
                }
                Status {
                    id: s.id,
                    app_id: s.manifest.id.clone(),
                    state: s.state,
                    cpu_pct: s.cpu_pct,
                    mem_kib: s.mem_kib,
                }
            })
            .collect()
    })
}

// ---------------------------------------------------------------- drawing (compositor)

const BG: crate::fb::Color = crate::fb::Color::rgb(0x10, 0x14, 0x20);
const FG: crate::fb::Color = crate::fb::Color::rgb(0x9a, 0xa6, 0xbd);
const ERR: crate::fb::Color = crate::fb::Color::rgb(0xe0, 0x70, 0x70);

enum Snap {
    Frame(*const u8, usize, i32, i32, usize),
    Message(String, bool),
}

/// Draw the app's latest frame into `c` at content origin `(cx, cy)`, clipped to a
/// `cw` x `ch` box; or a message panel while it loads or after it ended.
pub fn blit(id: AppId, c: &mut crate::fb::Canvas, cx: i32, cy: i32, cw: i32, ch: i32) {
    let snap = with(|slots| {
        let Some(s) = slot_of(slots, id) else {
            return Snap::Message(String::from("App nao encontrado"), true);
        };
        match s.state {
            State::Crashed => {
                return Snap::Message(alloc::format!("O app encerrou: {}", s.reason), true);
            }
            State::Exited => {
                return Snap::Message(alloc::format!("O app encerrou: {}", s.reason), false);
            }
            _ => {}
        }
        if !s.ready || s.surf[s.front].is_empty() {
            return Snap::Message(String::from("Carregando app..."), false);
        }
        s.reading = Some(s.front);
        let f = &s.surf[s.front];
        Snap::Frame(f.as_ptr(), f.len(), s.cur_w, s.cur_h, s.front)
    });
    match snap {
        Snap::Message(text, is_err) => message(c, cx, cy, cw, ch, &text, is_err),
        Snap::Frame(ptr, len, sw, sh, idx) => {
            // SAFETY: `reading = Some(idx)` tells `appd` not to render into or reallocate this surface and
            // `reap` not to free it until we clear it below; the Vec's buffer is `len` bytes and stays put.
            let src = unsafe { core::slice::from_raw_parts(ptr, len) };
            copy_frame(c, src, sw, sh, cx, cy, cw, ch);
            with(|slots| {
                if let Some(s) = slot_of(slots, id)
                    && s.reading == Some(idx)
                {
                    s.reading = None;
                }
            });
        }
    }
}

fn message(c: &mut crate::fb::Canvas, cx: i32, cy: i32, cw: i32, ch: i32, text: &str, err: bool) {
    let (x, y) = (cx.max(0) as usize, cy.max(0) as usize);
    c.fill_rect(x, y, cw.max(0) as usize, ch.max(0) as usize, BG);
    let col = if err { ERR } else { FG };
    // word-wrap at the box width, scale 2 (16 px cells)
    let per_line = ((cw - 32).max(16) as usize / crate::text::guest::cell_w(2)).max(8);
    let mut line = String::new();
    let mut ty = cy + 16;
    for word in text.split(' ') {
        if !line.is_empty() && line.len() + 1 + word.len() > per_line {
            crate::text::guest::draw_text(
                c,
                (cx + 16).max(0) as usize,
                ty.max(0) as usize,
                &line,
                col,
                2,
            );
            ty += 22;
            line.clear();
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() && ty + 16 <= cy + ch {
        crate::text::guest::draw_text(
            c,
            (cx + 16).max(0) as usize,
            ty.max(0) as usize,
            &line,
            col,
            2,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn copy_frame(
    c: &mut crate::fb::Canvas,
    src: &[u8],
    sw: i32,
    sh: i32,
    cx: i32,
    cy: i32,
    cw: i32,
    ch: i32,
) {
    let info = c.fb_info();
    let bpp = info.bytes_per_pixel;
    let stride = info.stride;
    let (scr_w, scr_h) = (info.width as i32, info.height as i32);
    let copy_w = sw.min(cw);
    let copy_h = sh.min(ch);
    let dst = c.buffer_mut();
    for y in 0..copy_h {
        let dyy = cy + y;
        if dyy < 0 || dyy >= scr_h {
            continue;
        }
        let x0 = cx.max(0);
        let x1 = (cx + copy_w).min(scr_w);
        if x1 <= x0 {
            continue;
        }
        let cols = (x1 - x0) as usize;
        let so = (y as usize * sw as usize + (x0 - cx) as usize) * bpp;
        let dofs = (dyy as usize * stride + x0 as usize) * bpp;
        if so + cols * bpp <= src.len() && dofs + cols * bpp <= dst.len() {
            dst[dofs..dofs + cols * bpp].copy_from_slice(&src[so..so + cols * bpp]);
        }
    }
    // The part of the box the (stale-size) frame does not cover.
    if sw < cw {
        c.fill_rect(
            (cx + sw).max(0) as usize,
            cy.max(0) as usize,
            (cw - sw) as usize,
            ch.max(0) as usize,
            BG,
        );
    }
    if sh < ch {
        c.fill_rect(
            cx.max(0) as usize,
            (cy + sh).max(0) as usize,
            cw.min(sw).max(0) as usize,
            (ch - sh) as usize,
            BG,
        );
    }
}

// ---------------------------------------------------------------- appd

/// What `appd` decided to do with one slot this pass (a snapshot taken under the lock).
struct Job {
    starting: bool,
    v1: bool,
    quotas: Quotas,
    events: Vec<Ev>,
    redraw: bool,
    resize: Option<(i32, i32)>,
    tick: Option<i32>,
    frame: bool,
}

/// Is slot `s` ready to run a slice at `now`?
fn ready(s: &Slot, now: u64) -> bool {
    if s.closing || s.restart {
        return false;
    }
    match s.state {
        State::Starting => true,
        State::Running => {
            if !s.events.is_empty() || s.want_redraw {
                return true;
            }
            if (s.req_w, s.req_h) != (s.cur_w, s.cur_h)
                && now.saturating_sub(s.req_at) >= RESIZE_SETTLE_TICKS
            {
                return true;
            }
            if s.v1 {
                return now >= s.last_frame + FRAME_TICKS;
            }
            tick_due(s, now)
        }
        _ => false,
    }
}

fn tick_due(s: &Slot, now: u64) -> bool {
    let ms = s.manifest.tick_ms as u64;
    ms != 0 && now >= s.last_tick + ms.div_ceil(4)
}

/// Earliest tick at which some slot becomes ready on its own (`FOREVER` if none).
fn next_deadline() -> u64 {
    with(|slots| {
        let mut d = crate::sched::FOREVER;
        for s in slots.iter().flatten() {
            if s.closing || s.restart || s.state != State::Running {
                continue;
            }
            if s.v1 {
                d = d.min(s.last_frame + FRAME_TICKS);
            } else if s.manifest.tick_ms != 0 {
                d = d.min(s.last_tick + (s.manifest.tick_ms as u64).div_ceil(4));
            }
            if (s.req_w, s.req_h) != (s.cur_w, s.cur_h) {
                d = d.min(s.req_at + RESIZE_SETTLE_TICKS);
            }
        }
        d
    })
}

fn any_ready() -> bool {
    let now = crate::interrupts::ticks();
    with(|slots| {
        slots
            .iter()
            .flatten()
            .any(|s| ready(s, now) || s.closing || s.restart)
    })
}

/// `appd` entry point: the single owner of every wasm runtime.
pub extern "C" fn worker() -> ! {
    TID.store(crate::sched::current(), Ordering::Release);
    let mut start = 0usize;
    loop {
        housekeeping();
        let mut ran = false;
        for k in 0..MAX_APPS {
            if run_slice((start + k) % MAX_APPS) {
                ran = true;
            }
        }
        start = (start + 1) % MAX_APPS;
        if !ran {
            let deadline = next_deadline();
            crate::sched::block(deadline, || !any_ready());
        }
    }
}

/// Drop the runtime of closing/restarting apps (frees their memory at once).
fn housekeeping() {
    for i in 0..MAX_APPS {
        let act = with(|slots| match slots[i].as_mut() {
            Some(s) if s.closing && !s.rt_dropped => 1,
            Some(s) if s.restart && !s.closing => {
                s.restart = false;
                2
            }
            _ => 0,
        });
        match act {
            1 => {
                rts()[i] = None;
                with(|slots| {
                    if let Some(s) = slots[i].as_mut() {
                        s.rt_dropped = true;
                    }
                });
            }
            2 => {
                rts()[i] = None;
                with(|slots| {
                    if let Some(s) = slots[i].as_mut() {
                        s.state = State::Starting;
                        s.reason.clear();
                        s.events.clear();
                        s.want_redraw = true;
                        s.ready = false;
                        s.rt_dropped = true;
                        s.cur_w = 0;
                        s.cur_h = 0;
                        s.title.clear();
                    }
                });
            }
            _ => {}
        }
    }
}

/// End the app in slot `i`: drop its runtime now, remember why.
fn finish(i: usize, state: State, reason: String) {
    rts()[i] = None;
    with(|slots| {
        if let Some(s) = slots[i].as_mut() {
            serial_println!(
                "apps: `{}` {}: {}",
                s.manifest.id,
                if state == State::Crashed {
                    "crashed"
                } else {
                    "exited"
                },
                reason
            );
            s.state = state;
            s.reason = reason;
            s.rt_dropped = true;
            s.events.clear();
            s.want_redraw = false;
        }
    });
}

fn run_slice(i: usize) -> bool {
    let now = crate::interrupts::ticks();
    let job = with(|slots| {
        let s = slots[i].as_mut()?;
        if !ready(s, now) {
            return None;
        }
        let starting = s.state == State::Starting;
        let mut events = Vec::new();
        for _ in 0..SLICE_EVENTS {
            match s.events.pop_front() {
                Some(e) => events.push(e),
                None => break,
            }
        }
        let resize = ((s.req_w, s.req_h) != (s.cur_w, s.cur_h)
            && (starting || now.saturating_sub(s.req_at) >= RESIZE_SETTLE_TICKS))
            .then_some((s.req_w, s.req_h));
        let tick = (!starting && !s.v1 && tick_due(s, now)).then(|| {
            let dt = (now.saturating_sub(s.last_tick) * 4).min(i32::MAX as u64) as i32;
            s.last_tick = now;
            dt
        });
        let frame = s.v1 && now >= s.last_frame + FRAME_TICKS;
        let redraw = core::mem::take(&mut s.want_redraw);
        Some(Job {
            starting,
            v1: s.v1,
            quotas: s.quotas,
            events,
            redraw,
            resize,
            tick,
            frame,
        })
    });
    let Some(job) = job else {
        return false;
    };
    let t0 = io::rdtsc();

    // ---- start-up ----
    if job.starting {
        match build_runtime(i) {
            Ok(rt) => {
                // The window may have been closed while the module was loading (the slot
                // can even be gone already): then the new runtime is dropped right away.
                let alive = with(|slots| match slots[i].as_mut() {
                    Some(s) if !s.closing => {
                        s.state = if s.visible {
                            State::Running
                        } else {
                            State::Suspended
                        };
                        s.rt_dropped = false;
                        s.started_tick = now;
                        s.last_tick = now;
                        true
                    }
                    _ => false,
                });
                if !alive {
                    return true;
                }
                rts()[i] = Some(rt);
            }
            Err(why) => {
                finish(i, State::Crashed, why);
                return true;
            }
        }
    }

    // ---- surfaces ----
    if let Some((w, h)) = job.resize
        && !resize_surfaces(i, w, h)
    {
        finish(i, State::Crashed, String::from("sem memoria para a janela"));
        return true;
    }

    // ---- run the guest ----
    let outcome = run_guest(i, &job);
    let spent = io::rdtsc().wrapping_sub(t0);
    match outcome {
        Ok(rendered) => {
            if rendered {
                publish(i);
            }
            account(i, spent, now);
            sync_title(i);
        }
        Err(e) => {
            account(i, spent, now);
            let why = describe(&e);
            let state = if e.i32_exit_status().is_some() {
                State::Exited
            } else {
                State::Crashed
            };
            finish(i, state, why);
        }
    }
    true
}

mod runtime;
use runtime::{account, build_runtime, publish, resize_surfaces, run_guest, sync_title};
