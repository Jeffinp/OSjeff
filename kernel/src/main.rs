#![no_std]
#![no_main]
#![allow(static_mut_refs)]

mod fb;
mod font;
mod io;
mod ps2;
mod rtc;

use bootloader_api::{entry_point, BootInfo};
use core::panic::PanicInfo;
use fb::{Canvas, Color};

entry_point!(kernel_main);

// Back buffer for flicker-free rendering. Sized for up to 1920x1080x4.
const MAX_BYTES: usize = 1920 * 1080 * 4;
static mut BACK: [u8; MAX_BYTES] = [0; MAX_BYTES];

struct Window {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    dragging: bool,
    grab_dx: i32,
    grab_dy: i32,
}

fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    let framebuffer = match boot_info.framebuffer.as_mut() {
        Some(fb) => fb,
        None => halt(),
    };
    let info = framebuffer.info();
    let len = framebuffer.buffer().len();

    // No heap: render into a fixed static back buffer, then blit.
    let back: &mut [u8] = unsafe {
        let ptr = core::ptr::addr_of_mut!(BACK) as *mut u8;
        core::slice::from_raw_parts_mut(ptr, len.min(MAX_BYTES))
    };

    ps2::init();

    let sw = info.width as i32;
    let sh = info.height as i32;

    let mut cursor_x = sw / 2;
    let mut cursor_y = sh / 2;
    let mut prev_left = false;

    let mut win = Window {
        x: sw / 2 - 210,
        y: sh / 2 - 150,
        w: 420,
        h: 260,
        dragging: false,
        grab_dx: 0,
        grab_dy: 0,
    };

    let title_h = 30i32;

    loop {
        // --- Input ---
        if let Some(p) = ps2::poll() {
            cursor_x = (cursor_x + p.dx).clamp(0, sw - 1);
            cursor_y = (cursor_y - p.dy).clamp(0, sh - 1); // mouse Y is inverted

            let pressed = p.left && !prev_left;
            let released = !p.left && prev_left;

            if pressed
                && cursor_x >= win.x
                && cursor_x < win.x + win.w
                && cursor_y >= win.y
                && cursor_y < win.y + title_h
            {
                win.dragging = true;
                win.grab_dx = cursor_x - win.x;
                win.grab_dy = cursor_y - win.y;
            }
            if released {
                win.dragging = false;
            }
            if win.dragging && p.left {
                win.x = (cursor_x - win.grab_dx).clamp(0, sw - win.w);
                win.y = (cursor_y - win.grab_dy).clamp(0, sh - title_h);
            }
            prev_left = p.left;
        }

        // --- Render scene into back buffer ---
        {
            let mut c = Canvas::new(back, info);
            draw_desktop(&mut c);
            draw_window(&mut c, &win, title_h);
            draw_cursor(&mut c, cursor_x as usize, cursor_y as usize);
        }

        // --- Blit back buffer to the real framebuffer ---
        framebuffer.buffer_mut()[..back.len()].copy_from_slice(back);
    }
}

fn draw_desktop(c: &mut Canvas) {
    let w = c.width();
    let h = c.height();

    // Wallpaper: vertical blue gradient.
    let top = Color::rgb(0x10, 0x2A, 0x52);
    let bottom = Color::rgb(0x2B, 0x6F, 0xD6);
    for y in 0..h {
        let t = ((y * 255) / h.max(1)) as u16;
        c.fill_rect(0, y, w, 1, top.lerp(bottom, t));
    }

    // Taskbar.
    let bar_h = 48usize;
    let bar_y = h.saturating_sub(bar_h);
    c.fill_rect(0, bar_y, w, bar_h, Color::rgb(0x20, 0x20, 0x28));

    // Brand on the left.
    font::draw_text(c, 14, bar_y + 16, "OSJEFF", Color::rgb(0xE6, 0xED, 0xFF), 2);

    // Centered icon group: start logo + app icons.
    let icon = 32usize;
    let gap = 12usize;
    let count = 5usize;
    let group_w = count * icon + (count - 1) * gap;
    let mut x = w / 2 - group_w / 2;
    let y = bar_y + (bar_h - icon) / 2;

    draw_start_logo(c, x, y, icon);
    x += icon + gap;
    for color in [
        Color::rgb(0x3B, 0x82, 0xF6),
        Color::rgb(0x22, 0xC5, 0x5E),
        Color::rgb(0xF5, 0x9E, 0x0B),
        Color::rgb(0xEF, 0x44, 0x44),
    ] {
        c.fill_round_rect(x, y, icon, icon, 8, color);
        x += icon + gap;
    }

    // Clock on the right (real time from RTC, UTC).
    let t = rtc::now();
    let mut buf = [b'0'; 8]; // "HH:MM:SS"
    write_two(&mut buf, 0, t.h);
    buf[2] = b':';
    write_two(&mut buf, 3, t.m);
    buf[5] = b':';
    write_two(&mut buf, 6, t.s);
    let clock = unsafe { core::str::from_utf8_unchecked(&buf) };
    let cw = font::text_width(clock, 2);
    font::draw_text(c, w - cw - 16, bar_y + 16, clock, Color::rgb(0xE6, 0xED, 0xFF), 2);
}

fn draw_start_logo(c: &mut Canvas, x: usize, y: usize, icon: usize) {
    let blue = Color::rgb(0x2D, 0x7D, 0xF6);
    let gap = 3usize;
    let s = (icon - gap) / 2;
    for (dx, dy) in [(0, 0), (s + gap, 0), (0, s + gap), (s + gap, s + gap)] {
        c.fill_round_rect(x + dx, y + dy, s, s, 2, blue);
    }
}

fn draw_window(c: &mut Canvas, win: &Window, title_h: i32) {
    let x = win.x as usize;
    let y = win.y as usize;
    let w = win.w as usize;
    let h = win.h as usize;
    let th = title_h as usize;

    // Body + title bar.
    c.fill_round_rect(x, y, w, h, 10, Color::rgb(0xF2, 0xF4, 0xF8));
    c.fill_rect(x, y + 6, w, th - 6, Color::rgb(0x1F, 0x53, 0xA8));
    c.fill_round_rect(x, y, w, th, 10, Color::rgb(0x1F, 0x53, 0xA8));

    // Title text.
    font::draw_text(c, x + 10, y + 8, "OSJEFF TERMINAL", Color::rgb(0xFF, 0xFF, 0xFF), 2);

    // Close button (decorative).
    let b = 18usize;
    c.fill_round_rect(x + w - b - 8, y + 6, b, b, 4, Color::rgb(0xE8, 0x4C, 0x3D));

    // Body text.
    let tx = x + 16;
    font::draw_text(c, tx, y + th + 16, "WELCOME TO OSJEFF", Color::rgb(0x12, 0x18, 0x28), 2);
    font::draw_text(c, tx, y + th + 44, "RUST OS - DRAG ME", Color::rgb(0x3A, 0x44, 0x55), 2);
    font::draw_text(c, tx, y + th + 80, "DOUBLE BUFFERED", Color::rgb(0x3A, 0x44, 0x55), 2);
    font::draw_text(c, tx, y + th + 104, "PS2 MOUSE - RTC CLOCK", Color::rgb(0x3A, 0x44, 0x55), 2);
}

// Classic arrow cursor. '#' = outline, '.' = fill, ' ' = transparent.
const CURSOR: [&str; 16] = [
    "#         ",
    "##        ",
    "#.#       ",
    "#..#      ",
    "#...#     ",
    "#....#    ",
    "#.....#   ",
    "#......#  ",
    "#.......# ",
    "#....#####",
    "#..#.#    ",
    "#.# #.#   ",
    "##  #.#   ",
    "#    #.#  ",
    "     #.#  ",
    "      #   ",
];

fn draw_cursor(c: &mut Canvas, px: usize, py: usize) {
    let outline = Color::rgb(0x10, 0x10, 0x10);
    let fill = Color::rgb(0xFF, 0xFF, 0xFF);
    for (row, line) in CURSOR.iter().enumerate() {
        for (col, ch) in line.bytes().enumerate() {
            let color = match ch {
                b'#' => outline,
                b'.' => fill,
                _ => continue,
            };
            c.put(px + col, py + row, color);
        }
    }
}

fn write_two(buf: &mut [u8], idx: usize, val: u8) {
    buf[idx] = b'0' + (val / 10) % 10;
    buf[idx + 1] = b'0' + val % 10;
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
