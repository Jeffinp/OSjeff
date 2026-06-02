#![no_std]
#![no_main]

mod fb;

use bootloader_api::{entry_point, BootInfo};
use core::panic::PanicInfo;
use fb::{Canvas, Color};

entry_point!(kernel_main);

fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    if let Some(framebuffer) = boot_info.framebuffer.as_mut() {
        let info = framebuffer.info();
        let mut canvas = Canvas::new(framebuffer.buffer_mut(), info);
        draw_desktop(&mut canvas);
    }
    halt();
}

/// Renders a static Windows 11-style desktop: gradient wallpaper + centered taskbar.
fn draw_desktop(c: &mut Canvas) {
    let w = c.width();
    let h = c.height();

    // --- Wallpaper: vertical blue gradient (Win11 "bloom" feel) ---
    let top = Color::rgb(0x10, 0x2A, 0x52);
    let bottom = Color::rgb(0x2B, 0x6F, 0xD6);
    for y in 0..h {
        let t = ((y * 255) / h.max(1)) as u16;
        let row = top.lerp(bottom, t);
        c.fill_rect(0, y, w, 1, row);
    }

    // --- Taskbar: full-width bar at the bottom ---
    let bar_h = 48usize;
    let bar_y = h.saturating_sub(bar_h);
    let bar_color = Color::rgb(0x20, 0x20, 0x28);
    c.fill_rect(0, bar_y, w, bar_h, bar_color);

    // --- Centered icon group: Start logo + app icons ---
    let icon = 32usize;
    let gap = 12usize;
    let count = 5usize; // start + 4 apps
    let group_w = count * icon + (count - 1) * gap;
    let mut x = w / 2 - group_w / 2;
    let y = bar_y + (bar_h - icon) / 2;

    draw_start_logo(c, x, y, icon);
    x += icon + gap;

    let app_colors = [
        Color::rgb(0x3B, 0x82, 0xF6), // blue
        Color::rgb(0x22, 0xC5, 0x5E), // green
        Color::rgb(0xF5, 0x9E, 0x0B), // amber
        Color::rgb(0xEF, 0x44, 0x44), // red
    ];
    for color in app_colors {
        c.fill_round_rect(x, y, icon, icon, 8, color);
        x += icon + gap;
    }
}

/// Windows-style four-square logo inside an `icon`-sized box.
fn draw_start_logo(c: &mut Canvas, x: usize, y: usize, icon: usize) {
    let blue = Color::rgb(0x2D, 0x7D, 0xF6);
    let gap = 3usize;
    let s = (icon - gap) / 2; // square side
    let positions = [(0, 0), (s + gap, 0), (0, s + gap), (s + gap, s + gap)];
    for (dx, dy) in positions {
        c.fill_round_rect(x + dx, y + dy, s, s, 2, blue);
    }
}

fn halt() -> ! {
    loop {
        // Park the CPU until the next interrupt instead of busy-spinning.
        unsafe { core::arch::asm!("hlt", options(nomem, nostack, preserves_flags)) };
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    halt();
}
