//! `plasma` — a native OSjeff app, written in Rust and compiled to WebAssembly.
//!
//! It computes an animated pattern into its own linear-memory framebuffer each
//! frame and hands it to the OS with `host.blit`, the way a real game (DOOM)
//! would push its rendered frame. State lives in the guest across frames, and it
//! reacts to keyboard/pointer events — proving the full pipeline: a real
//! compiled language → `.wasm` → a native, interactive, framebuffer app.

#![no_std]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {}
}

// The OS ABI (host imports). `host.blit` copies an RGBA image from guest memory
// to the window; `host.fill_rect`/`host.draw_text` draw chrome; `host.time_ms`
// drives the animation.
#[link(wasm_import_module = "host")]
extern "C" {
    fn blit(ptr: *const u8, w: i32, h: i32, dx: i32, dy: i32);
    fn fill_rect(x: i32, y: i32, w: i32, h: i32, rgb: i32);
    fn draw_text(x: i32, y: i32, ptr: *const u8, len: i32, rgb: i32, scale: i32);
    fn time_ms() -> i64;
}

const W: usize = 320;
const H: usize = 180;

static TITLE: &[u8] = b"plasma.wasm  -  Rust compilado para WebAssembly";

// The app's own framebuffer + interactive state, in its linear memory. Persisted
// across frames because the engine keeps the instance resident.
static mut FB: [u8; W * H * 4] = [0; W * H * 4];
static mut SHIFT: u32 = 0; // palette offset, nudged by input
static mut SPEED: u32 = 16; // animation divisor, cycled by Enter

/// Render one frame: a header bar, then the computed plasma image blitted below.
#[no_mangle]
pub extern "C" fn render() {
    let t = unsafe { time_ms() } as u32;
    let shift = unsafe { SHIFT };
    let speed = unsafe { SPEED }.max(1);

    unsafe {
        fill_rect(0, 0, 960, 40, 0x654FF0);
        draw_text(14, 9, TITLE.as_ptr(), TITLE.len() as i32, 0xFFFFFF, 2);
    }

    let fb = unsafe { &mut *core::ptr::addr_of_mut!(FB) };
    let phase = t / speed;
    let mut i = 0usize;
    let mut y = 0u32;
    while (y as usize) < H {
        let mut x = 0u32;
        while (x as usize) < W {
            // Cheap concentric/diagonal interference — no float, no libm.
            let a = x.wrapping_mul(x).wrapping_add(y.wrapping_mul(y));
            let b = x.wrapping_add(y).wrapping_add(phase);
            let v = a.wrapping_add(phase).wrapping_add(shift);
            fb[i] = v as u8;
            fb[i + 1] = b as u8;
            fb[i + 2] = (v.wrapping_add(b) >> 1) as u8;
            fb[i + 3] = 255;
            i += 4;
            x += 1;
        }
        y += 1;
    }
    unsafe { blit(fb.as_ptr(), W as i32, H as i32, 8, 56) };
}

/// Keyboard: Space shifts the palette, Enter cycles the animation speed.
#[no_mangle]
pub extern "C" fn on_key(code: i32) {
    unsafe {
        if code == 32 {
            SHIFT = SHIFT.wrapping_add(48);
        } else if code == 10 {
            SPEED = if SPEED >= 40 { 4 } else { SPEED + 8 };
        }
    }
}

/// Pointer: a click shifts the palette by the click's x, so it feels reactive.
#[no_mangle]
pub extern "C" fn on_pointer(x: i32, _y: i32, buttons: i32) {
    if buttons != 0 {
        unsafe { SHIFT = SHIFT.wrapping_add((x.max(0) as u32).wrapping_mul(2)) };
    }
}
