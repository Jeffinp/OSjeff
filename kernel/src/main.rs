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
mod ps2;
mod rtc;
mod theme;

use bootloader_api::info::FrameBufferInfo;
use bootloader_api::{entry_point, BootInfo};
use core::panic::PanicInfo;
use desktop::{Desktop, CURSOR_H, CURSOR_W};
use fb::Canvas;
use osjeff_core::Time;
use ps2::Event;

entry_point!(kernel_main);

// Render buffers sized for up to 1920x1080x4. BACK is the compositing target;
// BG caches the static wallpaper so it is never recomputed per frame.
const MAX_BYTES: usize = 1920 * 1080 * 4;
static mut BACK: [u8; MAX_BYTES] = [0; MAX_BYTES];
static mut BG: [u8; MAX_BYTES] = [0; MAX_BYTES];

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

    // Initialize the kernel heap so `alloc` works, then smoke-test it.
    unsafe {
        ALLOCATOR.init(core::ptr::addr_of_mut!(HEAP) as usize, HEAP_SIZE);
    }
    heap_smoke_test();

    // Real interrupts: IDT + exception handlers, PIC remap, PIT timer.
    // Only IRQ0 (timer) is unmasked; input stays on polling.
    interrupts::init();

    // Boot splash: progress tracks real elapsed time (>= 5 seconds).
    run_splash(&mut *framebuffer, &mut *back, info, n);

    // Static layer painted once.
    {
        let mut c = Canvas::new(bg, info);
        desktop::paint_background(&mut c);
    }

    ps2::init();
    let mut desk = Desktop::new(info.width as i32, info.height as i32);
    let mut last_sec = 0xFFu8; // force first render
    let mut prev_cursor = desk.cursor();

    // Animation timestep per rendered frame, and the per-frame pacing delay
    // (~6ms) that keeps transitions visible without a timer interrupt.
    const DT: f32 = 0.08;
    const FRAME_CYCLES: u64 = 18_000_000;

    loop {
        let rt = rtc::now();
        let time = Time {
            h: rt.h,
            m: rt.m,
            s: rt.s,
        };

        let animating = desk.animate(DT);
        let mut scene_dirty = animating;
        let mut cursor_moved = false;

        // Drain all pending PS/2 events so the cursor stays responsive.
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

        if scene_dirty {
            // Full recompose: scene (without cursor) -> back -> framebuffer,
            // then the cursor as an overlay on top.
            back.copy_from_slice(bg);
            desk.render(back, info, time);
            framebuffer.buffer_mut()[..n].copy_from_slice(back);
            {
                let mut c = Canvas::new(&mut framebuffer.buffer_mut()[..n], info);
                desk.draw_cursor_overlay(&mut c);
            }
            prev_cursor = desk.cursor();
            if animating {
                io::delay_cycles(FRAME_CYCLES);
            }
        } else if cursor_moved {
            // Cheap path: restore the pixels under the old cursor from `back`
            // (which holds the cursor-free scene), then draw the cursor at its
            // new position. No full-screen copy.
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

/// Exercises the heap (alloc, grow, free) once at boot. A broken allocator
/// would fault or hang here instead of silently corrupting later.
fn heap_smoke_test() {
    use alloc::vec::Vec;
    let mut v: Vec<u32> = Vec::new();
    for i in 0..1024 {
        v.push(i);
    }
    let sum: u32 = v.iter().sum();
    core::hint::black_box(sum);
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
