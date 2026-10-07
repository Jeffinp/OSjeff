//! Fatal-error reporting: COM1 line plus an on-screen error page.
//!
//! Until now a panic or fatal CPU exception printed on the serial port and
//! parked the CPU, leaving a frozen desktop with no explanation for anyone not
//! watching COM1. [`die`] additionally paints a self-contained error screen
//! (dark background, red banner, the message, thread name and registers) straight
//! into the hardware framebuffer registered by [`register_framebuffer`].
//!
//! It is written to work when everything else is broken, so it uses no
//! allocation, no locks, no compositor/back buffers and no scheduler state other
//! than a plain read of the current thread's name. It is also re-entrancy safe:
//! a fault while painting the error screen falls through to a plain `cli; hlt`.

use crate::fb::{Canvas, Color};
use crate::font;
use crate::sync::RacyCell;
use bootloader_api::info::FrameBufferInfo;
use core::fmt::{self, Write};
use core::sync::atomic::{AtomicBool, Ordering};

/// What went wrong; selects the banner text.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Rust `panic!` (including allocation failure and the stack canary).
    Panic,
    /// A CPU exception with no recovery path.
    Exception,
    /// The machine's framebuffer is larger than the kernel's fixed buffers.
    Unsupported,
}

/// CPU state captured by an exception gate.
#[derive(Clone, Copy)]
pub struct Frame {
    pub rip: u64,
    pub rsp: u64,
    pub rflags: u64,
    pub code: Option<u64>,
}

/// The hardware framebuffer, as handed over by the bootloader.
struct Screen {
    ptr: *mut u8,
    len: usize,
    info: FrameBufferInfo,
}

static SCREEN: RacyCell<Option<Screen>> = RacyCell::new(None);
/// Set (Release) once `SCREEN` is fully written; `die` reads it (Acquire).
static SCREEN_READY: AtomicBool = AtomicBool::new(false);
/// Set by the first caller of [`die`]; a second entry (fault while painting) just halts.
static BUSY: AtomicBool = AtomicBool::new(false);

/// Remember the hardware framebuffer so [`die`] can paint on it later.
///
/// Call once from `kernel_main`, as soon as the bootloader's framebuffer is
/// known and before anything can fail.
pub fn register_framebuffer(buf: &mut [u8], info: FrameBufferInfo) {
    // SAFETY: boot-only and called once, on the boot thread, before `interrupts::init` enables IF and
    // before any other thread exists; `die` reads `SCREEN` only after seeing `SCREEN_READY`, which is
    // published (Release) after this write. `buf` is the bootloader's framebuffer mapping, valid for
    // the whole run.
    unsafe {
        *SCREEN.get() = Some(Screen {
            ptr: buf.as_mut_ptr(),
            len: buf.len(),
            info,
        });
    }
    SCREEN_READY.store(true, Ordering::Release);
}

/// Disable interrupts and park the CPU forever.
pub fn halt() -> ! {
    x86_64::instructions::interrupts::disable();
    loop {
        x86_64::instructions::hlt();
    }
}

/// Handle a failure in the running thread: if it can be contained (see
/// [`crate::sched::containable`]) kill just that thread and keep the machine
/// running; otherwise fall through to [`die`] (error screen, halt).
///
/// `if_was_set` is the interrupt flag of the failing context: RFLAGS.IF of the
/// faulting code for an exception, the live flag for a panic. Never returns.
pub fn fault(
    kind: Kind,
    subtitle: &str,
    msg: fmt::Arguments<'_>,
    frame: Option<Frame>,
    if_was_set: bool,
) -> ! {
    if kind != Kind::Unsupported && crate::sched::containable(if_was_set) {
        match frame {
            Some(f) => crate::sched::kill_current(format_args!(
                "{subtitle}: {msg} (rip={:#x} rsp={:#x} code={:?} cr2={:#x})",
                f.rip,
                f.rsp,
                f.code,
                x86_64::registers::control::Cr2::read_raw()
            )),
            // A panic message already says "panicked at <location>: <text>".
            None if kind == Kind::Panic => crate::sched::kill_current(format_args!("{msg}")),
            None => crate::sched::kill_current(format_args!("{subtitle}: {msg}")),
        }
    }
    die(kind, subtitle, msg, frame)
}

/// Report a fatal condition on COM1 and on the screen, then halt. Never returns.
///
/// `subtitle` is the exception name (`"#PF page fault"`) or a short reason;
/// `msg` is the human-readable detail (the panic message and location).
pub fn die(kind: Kind, subtitle: &str, msg: fmt::Arguments<'_>, frame: Option<Frame>) -> ! {
    // Stop the world first: no timer tick may switch away while we paint.
    x86_64::instructions::interrupts::disable();
    if BUSY.swap(true, Ordering::AcqRel) {
        halt(); // fault while reporting a fault
    }

    let cr2 = x86_64::registers::control::Cr2::read_raw();
    match (kind, frame) {
        (Kind::Panic, _) => crate::serial_println!("KERNEL PANIC: {msg}"),
        (Kind::Unsupported, _) => crate::serial_println!("FATAL: {subtitle}: {msg}"),
        (Kind::Exception, Some(f)) => crate::serial_println!(
            "FATAL EXCEPTION: {subtitle} code={:?} rip={:#x} rsp={:#x} rflags={:#x} cr2={cr2:#x} {msg}",
            f.code,
            f.rip,
            f.rsp,
            f.rflags
        ),
        (Kind::Exception, None) => crate::serial_println!("FATAL EXCEPTION: {subtitle} {msg}"),
    }

    if SCREEN_READY.load(Ordering::Acquire) {
        // SAFETY: `SCREEN_READY` (Acquire) means `register_framebuffer` finished its write and nobody
        // writes `SCREEN` again.
        if let Some(screen) = unsafe { (*SCREEN.get()).as_ref() } {
            paint(screen, kind, subtitle, msg, frame, cr2);
        }
    }
    halt()
}

const BG: Color = Color::rgb(18, 20, 32);
const BANNER: Color = Color::rgb(190, 36, 48);
const WHITE: Color = Color::rgb(236, 238, 246);
const AMBER: Color = Color::rgb(255, 196, 92);
const DIM: Color = Color::rgb(150, 156, 176);

/// Word-wrapping text writer over a [`Canvas`] (no allocation).
struct Console<'a> {
    canvas: Canvas<'a>,
    x: usize,
    y: usize,
    left: usize,
    right: usize,
    bottom: usize,
    scale: usize,
    fg: Color,
}

impl Console<'_> {
    fn newline(&mut self) {
        self.x = self.left;
        self.y += 10 * self.scale;
    }
}

impl fmt::Write for Console<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let cell = font::cell_w(self.scale);
        for ch in s.chars() {
            match ch {
                '\n' => self.newline(),
                '\r' => {}
                _ => {
                    let b = if ch.is_ascii() && !ch.is_ascii_control() {
                        ch as u8
                    } else {
                        b'?'
                    };
                    if self.x + cell > self.right {
                        self.newline();
                    }
                    if self.y + 8 * self.scale <= self.bottom {
                        font::draw_char(&mut self.canvas, self.x, self.y, b, self.fg, self.scale);
                    }
                    self.x += cell;
                }
            }
        }
        Ok(())
    }
}

fn paint(
    screen: &Screen,
    kind: Kind,
    subtitle: &str,
    msg: fmt::Arguments<'_>,
    frame: Option<Frame>,
    cr2: u64,
) {
    let info = screen.info;
    let (w, h) = (info.width, info.height);
    if w == 0 || h == 0 || info.stride < w || info.bytes_per_pixel == 0 {
        return;
    }
    // Bytes the canvas may touch; refuse rather than index out of the buffer.
    if ((h - 1) * info.stride + w) * info.bytes_per_pixel > screen.len {
        return;
    }
    // SAFETY: `ptr`/`len` describe the bootloader's framebuffer mapping (see `register_framebuffer`).
    // The boot code still holds its own `&mut` to the same memory, so strictly this aliases; that is
    // acceptable only because we are the last code that will ever run: IF is clear, this is a
    // single-core kernel, and `die` never returns.
    let buf = unsafe { core::slice::from_raw_parts_mut(screen.ptr, screen.len) };
    let mut canvas = Canvas::new(buf, info);

    let scale = (w / 640).clamp(1, 4);
    let tscale = scale * 2;
    let margin = 3 * font::cell_w(scale);
    let banner_h = 12 * tscale;

    canvas.fill_rect(0, 0, w, h, BG);
    canvas.fill_rect(0, 0, w, banner_h, BANNER);
    let title = match kind {
        Kind::Panic => "KERNEL PANIC",
        Kind::Exception => "CPU EXCEPTION",
        Kind::Unsupported => "UNSUPPORTED SCREEN",
    };
    font::draw_text(&mut canvas, margin, 2 * tscale, title, WHITE, tscale);

    let mut con = Console {
        canvas,
        x: margin,
        y: banner_h + 20 * scale,
        left: margin,
        right: w.saturating_sub(margin),
        bottom: h.saturating_sub(10 * scale),
        scale,
        fg: AMBER,
    };
    let _ = write!(con, "{subtitle}\n\n");
    con.fg = WHITE;
    let _ = write!(con, "{msg}\n\n");

    if kind != Kind::Unsupported {
        con.fg = DIM;
        let name = crate::sched::thread_name(crate::sched::current());
        let _ = writeln!(
            con,
            "thread : {}",
            if name.is_empty() { "boot" } else { name }
        );
        let rsp = match frame {
            Some(f) => f.rsp,
            None => current_rsp(),
        };
        if let Some(f) = frame {
            let _ = writeln!(con, "RIP    : {:#018x}", f.rip);
        }
        let _ = writeln!(con, "RSP    : {rsp:#018x}");
        if let Some(f) = frame {
            let _ = writeln!(con, "RFLAGS : {:#018x}", f.rflags);
            if let Some(code) = f.code {
                let _ = writeln!(con, "ERROR  : {code:#018x}");
            }
        }
        let _ = writeln!(con, "CR2    : {cr2:#018x}");
        con.newline();
    }
    con.fg = DIM;
    let _ = write!(
        con,
        "The system is halted. Reset or power-cycle the machine to restart.\n\
         The same report was written to the serial port (COM1)."
    );
}

fn current_rsp() -> u64 {
    let rsp: u64;
    // SAFETY: reads RSP into a register; no memory access, no stack use, no flags change.
    unsafe {
        core::arch::asm!("mov {}, rsp", out(reg) rsp, options(nomem, nostack, preserves_flags));
    }
    rsp
}
