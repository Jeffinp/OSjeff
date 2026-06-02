#![no_std]
#![no_main]
#![allow(static_mut_refs)]

mod desktop;
mod fb;
mod font;
mod io;
mod ps2;
mod rtc;

use bootloader_api::{entry_point, BootInfo};
use core::panic::PanicInfo;
use desktop::Desktop;
use fb::Canvas;
use osjeff_core::Time;
use ps2::Event;

entry_point!(kernel_main);

// Render buffers sized for up to 1920x1080x4. BACK is the compositing target;
// BG caches the static wallpaper so it is never recomputed per frame.
const MAX_BYTES: usize = 1920 * 1080 * 4;
static mut BACK: [u8; MAX_BYTES] = [0; MAX_BYTES];
static mut BG: [u8; MAX_BYTES] = [0; MAX_BYTES];

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

    // Static layer painted once.
    {
        let mut c = Canvas::new(bg, info);
        desktop::paint_background(&mut c);
    }

    ps2::init();
    let mut desk = Desktop::new(info.width as i32, info.height as i32);
    let mut last_sec = 0xFFu8; // force first render

    loop {
        let rt = rtc::now();
        let time = Time {
            h: rt.h,
            m: rt.m,
            s: rt.s,
        };

        // Drain all pending PS/2 events so the cursor stays responsive.
        let mut dirty = false;
        while let Some(event) = ps2::poll() {
            dirty |= match event {
                Event::Mouse(p) => desk.handle_mouse(p.dx, p.dy, p.left),
                Event::Key(k) => desk.handle_key(k.scan_code, k.extended, k.pressed, time),
            };
        }

        if rt.s != last_sec {
            last_sec = rt.s;
            dirty = true;
        }
        if !dirty {
            continue;
        }

        back.copy_from_slice(bg);
        {
            let mut c = Canvas::new(back, info);
            desk.render(&mut c, time);
        }
        framebuffer.buffer_mut()[..n].copy_from_slice(back);
    }
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
