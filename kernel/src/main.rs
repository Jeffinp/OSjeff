#![no_std]
#![no_main]
#![allow(static_mut_refs)]
#![feature(abi_x86_interrupt)]

extern crate alloc;

mod allocator;
mod boot;
mod desktop;
mod fb;
mod font;
mod icons;
mod interrupts;
mod io;
mod logo;
mod power;
mod ps2;
mod rtc;
mod sched;
mod theme;

use bootloader_api::info::FrameBufferInfo;
use bootloader_api::{entry_point, BootInfo};
use core::panic::PanicInfo;
use desktop::{Desktop, CURSOR_H, CURSOR_W};
use fb::Canvas;
use osjeff_core::{Rect, Time};
use ps2::Event;

entry_point!(kernel_main);

// Render buffers sized for up to 1920x1080x4. BACK is the compositing target;
// BG caches the static wallpaper so it is never recomputed per frame.
const MAX_BYTES: usize = 1920 * 1080 * 4;

// 64-byte aligned so the framebuffer fast paths can reinterpret rows as `[u32]`
// (one 32-bit store per pixel, vectorized) without an unaligned cast.
#[repr(C, align(64))]
struct AlignedBuf([u8; MAX_BYTES]);

static mut BACK: AlignedBuf = AlignedBuf([0; MAX_BYTES]);
static mut BG: AlignedBuf = AlignedBuf([0; MAX_BYTES]);
// Cached "everything except the animating window(s)" layer, composed once per
// animation so each frame only redraws the small damaged region.
static mut STATIC: AlignedBuf = AlignedBuf([0; MAX_BYTES]);

// Kernel heap (1 MiB) backing the global allocator, so `alloc` works.
const HEAP_SIZE: usize = 1024 * 1024;
static mut HEAP: [u8; HEAP_SIZE] = [0; HEAP_SIZE];

#[global_allocator]
static ALLOCATOR: allocator::LockedHeap = allocator::LockedHeap::new();

fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    let framebuffer = match boot_info.framebuffer.as_mut() {
        Some(fb) => fb,
        None => halt(),
    };
    let info = framebuffer.info();
    let n = framebuffer.buffer().len().min(MAX_BYTES);

    let back: &mut [u8] =
        unsafe { core::slice::from_raw_parts_mut(core::ptr::addr_of_mut!(BACK) as *mut u8, n) };
    let bg: &mut [u8] =
        unsafe { core::slice::from_raw_parts_mut(core::ptr::addr_of_mut!(BG) as *mut u8, n) };
    let static_buf: &mut [u8] =
        unsafe { core::slice::from_raw_parts_mut(core::ptr::addr_of_mut!(STATIC) as *mut u8, n) };

    // Initialize the kernel heap so `alloc` works, then smoke-test it.
    unsafe {
        ALLOCATOR.init(core::ptr::addr_of_mut!(HEAP) as usize, HEAP_SIZE);
    }
    heap_smoke_test();

    // Kernel scheduler: register the boot context as a thread and spawn a
    // couple of background workers that run concurrently with the GUI.
    sched::init();
    sched::spawn("worker-a", worker_a);
    sched::spawn("worker-b", worker_b);

    // Configure the PS/2 controller BEFORE enabling interrupts, so the init
    // handshake (config write + mouse ACKs) is read by polling without racing
    // the IRQ handlers.
    ps2::init();

    // Real interrupts: IDT + exception handlers, PIC remap, PIT timer, and
    // IRQ-driven keyboard (IRQ1) + mouse (IRQ12).
    interrupts::init();

    // Boot splash: progress tracks real elapsed time (>= 5 seconds).
    run_splash(&mut *framebuffer, &mut *back, info, n);

    // Static layer painted once.
    {
        let mut c = Canvas::new(bg, info);
        desktop::paint_background(&mut c);
    }

    let mut desk = Desktop::new(info.width as i32, info.height as i32);
    let mut last_sec = 0xFFu8; // force first render
    let mut prev_cursor = desk.cursor();
    let mut last_tick = 0u64;

    // Animation fast-path state.
    let mut was_anim = false;
    let mut static_valid = false;
    let mut last_sig = 0u32;
    let mut prev_damage = Rect::new(0, 0, 0, 0);

    // Wall-clock animation speed, independent of how often the GUI thread is
    // scheduled (the timer preempts round-robin across all threads).
    const DT_PER_TICK: f32 = 0.03;

    loop {
        let rt = rtc::now();
        let time = Time {
            h: rt.h,
            m: rt.m,
            s: rt.s,
        };

        // Advance animation by real elapsed timer ticks.
        let tick = interrupts::ticks();
        let tick_delta = tick.saturating_sub(last_tick);
        last_tick = tick;
        let tick_changed = tick_delta > 0;
        if tick_changed {
            desk.animate(tick_delta as f32 * DT_PER_TICK);
        }

        // Drain all pending PS/2 events.
        let mut scene_dirty = false;
        let mut cursor_moved = false;
        while let Some(event) = ps2::poll() {
            match event {
                Event::Mouse(p) => {
                    let r = desk.handle_mouse(p.dx, p.dy, p.left, p.right);
                    scene_dirty |= r.scene_dirty;
                    cursor_moved |= r.cursor_moved;
                }
                Event::Key(k) => {
                    if desk.handle_key(k.scan_code, k.extended, k.pressed, time) {
                        scene_dirty = true;
                    }
                }
            }
        }

        if rt.s != last_sec {
            last_sec = rt.s;
            desk.tick_processes();
            scene_dirty = true;
        }

        let any_anim = desk.has_animation();

        if any_anim {
            // ---- Animation fast-path: cache the static scene, then each frame
            // only touch the small damaged region around the animating window.
            let sig = desk.anim_signature();
            if !static_valid || sig != last_sig || scene_dirty {
                static_buf.copy_from_slice(bg);
                desk.compose_static(static_buf, info, time);
                back.copy_from_slice(static_buf);
                // Draw the animating window(s) into `back` BEFORE the full blit
                // so the framebuffer never shows a frame with them missing.
                // Without this, closing a *visible* window blanks it for one
                // full-screen blit (a visible blink) before the damage pass
                // redraws it — the static layer excludes animating windows.
                let dmg = desk.render_anim_frame(back, static_buf, info, Rect::new(0, 0, 0, 0));
                framebuffer.buffer_mut()[..n].copy_from_slice(back);
                {
                    let mut c = Canvas::new(&mut framebuffer.buffer_mut()[..n], info);
                    desk.draw_cursor_overlay(&mut c);
                }
                prev_cursor = desk.cursor();
                static_valid = true;
                last_sig = sig;
                prev_damage = dmg;
            }

            if tick_changed || cursor_moved {
                let damage = desk.render_anim_frame(back, static_buf, info, prev_damage);
                blit_rect(
                    framebuffer.buffer_mut(),
                    back,
                    info,
                    damage.x,
                    damage.y,
                    damage.w,
                    damage.h,
                    n,
                );
                // Repaint the cursor (the damage blit may have covered it, and the
                // cursor itself may have moved).
                let (ox, oy) = prev_cursor;
                blit_rect(
                    framebuffer.buffer_mut(),
                    back,
                    info,
                    ox,
                    oy,
                    CURSOR_W,
                    CURSOR_H,
                    n,
                );
                {
                    let mut c = Canvas::new(&mut framebuffer.buffer_mut()[..n], info);
                    desk.draw_cursor_overlay(&mut c);
                }
                prev_cursor = desk.cursor();
                prev_damage = damage;
            }
        } else {
            static_valid = false;
            // A finished animation needs one final full recompose to settle.
            if was_anim {
                scene_dirty = true;
            }

            if scene_dirty {
                back.copy_from_slice(bg);
                desk.render(back, info, time);
                framebuffer.buffer_mut()[..n].copy_from_slice(back);
                {
                    let mut c = Canvas::new(&mut framebuffer.buffer_mut()[..n], info);
                    desk.draw_cursor_overlay(&mut c);
                }
                prev_cursor = desk.cursor();
            } else if cursor_moved {
                // Cheap path: restore under the old cursor, draw at the new spot.
                let (ox, oy) = prev_cursor;
                blit_rect(
                    framebuffer.buffer_mut(),
                    back,
                    info,
                    ox,
                    oy,
                    CURSOR_W,
                    CURSOR_H,
                    n,
                );
                {
                    let mut c = Canvas::new(&mut framebuffer.buffer_mut()[..n], info);
                    desk.draw_cursor_overlay(&mut c);
                }
                prev_cursor = desk.cursor();
            }
        }
        was_anim = any_anim;

        // Idle until the next interrupt instead of busy-spinning. The timer
        // (250 Hz) wakes us to step animations and the per-second clock; the
        // keyboard/mouse IRQs wake us immediately on input. This paces frames to
        // the tick rate and stops the compositor from burning a full core — a
        // real system halts when it has nothing to draw. (The demo workers still
        // spin without yielding, which is what proves preemption.)
        x86_64::instructions::hlt();
    }
}

fn secs_of_day(t: rtc::Time) -> u32 {
    t.h as u32 * 3600 + t.m as u32 * 60 + t.s as u32
}

/// Plays the boot splash, driving the progress bar from real elapsed RTC time
/// so it always lasts at least 5 seconds regardless of CPU speed.
fn run_splash(
    framebuffer: &mut bootloader_api::info::FrameBuffer,
    back: &mut [u8],
    info: FrameBufferInfo,
    n: usize,
) {
    let start = secs_of_day(rtc::now());
    let mut prev_el = 0u32;
    let mut frac = 0.0f32;
    loop {
        let el = (secs_of_day(rtc::now()) + 86_400 - start) % 86_400;
        if el != prev_el {
            frac = 0.0;
            prev_el = el;
        }
        let p = ((el as f32) + frac.min(0.99)) / 5.0;
        {
            let mut c = Canvas::new(back, info);
            boot::draw_splash(&mut c, p);
        }
        framebuffer.buffer_mut()[..n].copy_from_slice(back);
        io::delay_cycles(20_000_000);
        frac += 0.06;
        if el >= 5 {
            break;
        }
    }
}

/// Copy a rectangular region from `src` into `dst` (same framebuffer layout).
/// Used to restore the background under the moving cursor.
#[allow(clippy::too_many_arguments)]
fn blit_rect(
    dst: &mut [u8],
    src: &[u8],
    info: FrameBufferInfo,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    n: usize,
) {
    let bpp = info.bytes_per_pixel;
    let stride = info.stride;
    let x = x.max(0) as usize;
    let y = y.max(0) as usize;
    if x >= info.width || y >= info.height {
        return;
    }
    let x_end = (x + w as usize).min(info.width);
    let y_end = (y + h as usize).min(info.height);
    let row_len = (x_end - x) * bpp;
    for row in y..y_end {
        let off = (row * stride + x) * bpp;
        let end = off + row_len;
        if end <= n {
            dst[off..end].copy_from_slice(&src[off..end]);
        }
    }
}

// Background worker threads. They never yield — the timer ISR preempts them —
// which is exactly what proves the scheduler is preemptive. Visible in the Task
// Manager with their real accumulated CPU time.
extern "C" fn worker_a() -> ! {
    let mut acc: u64 = 0;
    loop {
        acc = acc.wrapping_add(1);
        core::hint::black_box(acc);
    }
}

extern "C" fn worker_b() -> ! {
    let mut acc: u64 = 1;
    loop {
        acc = acc.wrapping_mul(3);
        core::hint::black_box(acc);
    }
}

/// Exercises the heap (alloc, grow, free) once at boot. A broken allocator
/// would fault or hang instead of silently corrupting later.
fn heap_smoke_test() {
    use alloc::vec::Vec;
    let mut v: Vec<u32> = Vec::new();
    for i in 0..1024 {
        v.push(i);
    }
    let sum: u32 = v.iter().sum();
    core::hint::black_box(sum);
    drop(v);

    // Fragmentation check: churn many small allocations, free them, then demand
    // a block larger than any single freed chunk. This only succeeds if the
    // allocator coalesced the freed regions back together.
    let mut chunks: Vec<Vec<u8>> = Vec::new();
    for _ in 0..256 {
        chunks.push(alloc::vec![0u8; 1024]);
    }
    drop(chunks); // frees ~256 KiB in scattered chunks
    let big: Vec<u8> = alloc::vec![7u8; 200 * 1024];
    core::hint::black_box(big.len());
}

fn halt() -> ! {
    loop {
        unsafe { core::arch::asm!("hlt", options(nomem, nostack, preserves_flags)) };
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    halt();
}
