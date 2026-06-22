//! `snake` — a playable game, written in Rust and compiled to WebAssembly, run
//! natively by OSjeff's WASM app engine.
//!
//! The whole game lives in the guest's linear memory and runs off the OS's
//! continuous frame pump: `render` advances the simulation on its own clock
//! (`host.time_ms`) and draws via `host.fill_rect`; `on_key` steers with WASD
//! and restarts with R/Space. Proof that the platform hosts a real, interactive,
//! real-time game — no foreign OS, no emulation.

#![no_std]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {}
}

#[link(wasm_import_module = "host")]
extern "C" {
    fn fill_rect(x: i32, y: i32, w: i32, h: i32, rgb: i32);
    fn draw_text(x: i32, y: i32, ptr: *const u8, len: i32, rgb: i32, scale: i32);
    fn time_ms() -> i64;
}

const COLS: i32 = 28;
const ROWS: i32 = 15;
const CELL: i32 = 20;
const OX: i32 = 16; // field origin within the window content
const OY: i32 = 52;
const MAXLEN: usize = (COLS * ROWS) as usize;
const STEP: i64 = 140; // ms between moves

static TITLE: &[u8] = b"snake.wasm  -  jogo nativo em Rust (WASD, R reinicia)";
static OVER: &[u8] = b"GAME OVER  -  R reinicia";

// Game state, in the guest's own memory (persists across frames).
static mut BODY: [(i16, i16); MAXLEN] = [(0, 0); MAXLEN]; // [0] = head
static mut LEN: usize = 0;
static mut DX: i32 = 1;
static mut DY: i32 = 0;
static mut PDX: i32 = 1; // pending direction (applied on the next step)
static mut PDY: i32 = 0;
static mut FX: i32 = 0;
static mut FY: i32 = 0;
static mut ALIVE: bool = false;
static mut RNG: u32 = 0;
static mut LAST: i64 = 0;
static mut STARTED: bool = false;

fn rng_next() -> u32 {
    unsafe {
        RNG = RNG.wrapping_mul(1664525).wrapping_add(1013904223);
        RNG
    }
}

fn place_food() {
    unsafe {
        for _ in 0..128 {
            let x = (rng_next() % COLS as u32) as i32;
            let y = (rng_next() % ROWS as u32) as i32;
            let mut on = false;
            for &(bx, by) in BODY.iter().take(LEN) {
                if bx as i32 == x && by as i32 == y {
                    on = true;
                    break;
                }
            }
            if !on {
                FX = x;
                FY = y;
                return;
            }
        }
    }
}

fn reset() {
    unsafe {
        let (cx, cy) = (COLS / 2, ROWS / 2);
        BODY[0] = (cx as i16, cy as i16);
        BODY[1] = ((cx - 1) as i16, cy as i16);
        BODY[2] = ((cx - 2) as i16, cy as i16);
        LEN = 3;
        DX = 1;
        DY = 0;
        PDX = 1;
        PDY = 0;
        ALIVE = true;
        if RNG == 0 {
            RNG = (time_ms() as u32) | 1;
        }
        place_food();
        LAST = time_ms();
    }
}

fn step() {
    unsafe {
        DX = PDX;
        DY = PDY;
        let hx = BODY[0].0 as i32 + DX;
        let hy = BODY[0].1 as i32 + DY;
        if !(0..COLS).contains(&hx) || !(0..ROWS).contains(&hy) {
            ALIVE = false;
            return;
        }
        for &(bx, by) in BODY.iter().take(LEN) {
            if bx as i32 == hx && by as i32 == hy {
                ALIVE = false;
                return;
            }
        }
        let grow = hx == FX && hy == FY;
        let newlen = if grow { (LEN + 1).min(MAXLEN) } else { LEN };
        let mut i = newlen - 1;
        while i > 0 {
            BODY[i] = BODY[i - 1];
            i -= 1;
        }
        BODY[0] = (hx as i16, hy as i16);
        LEN = newlen;
        if grow {
            place_food();
        }
    }
}

#[no_mangle]
pub extern "C" fn on_key(code: i32) {
    unsafe {
        match code as u8 {
            b'w' | b'W' => {
                if DY == 0 {
                    PDX = 0;
                    PDY = -1;
                }
            }
            b's' | b'S' => {
                if DY == 0 {
                    PDX = 0;
                    PDY = 1;
                }
            }
            b'a' | b'A' => {
                if DX == 0 {
                    PDX = -1;
                    PDY = 0;
                }
            }
            b'd' | b'D' => {
                if DX == 0 {
                    PDX = 1;
                    PDY = 0;
                }
            }
            b'r' | b'R' | b' ' => {
                if !ALIVE {
                    reset();
                }
            }
            _ => {}
        }
    }
}

#[no_mangle]
pub extern "C" fn render() {
    unsafe {
        if !STARTED {
            STARTED = true;
            reset();
        }
        let now = time_ms();
        if ALIVE && now - LAST >= STEP {
            LAST = now;
            step();
        }

        fill_rect(0, 0, 960, 40, 0x654FF0);
        draw_text(14, 9, TITLE.as_ptr(), TITLE.len() as i32, 0xFFFFFF, 2);

        // Playfield.
        fill_rect(OX - 2, OY - 2, COLS * CELL + 4, ROWS * CELL + 4, 0x10182A);
        // Food.
        fill_rect(OX + FX * CELL + 2, OY + FY * CELL + 2, CELL - 4, CELL - 4, 0xE54B4B);
        // Snake (brighter head).
        for (i, &(bx, by)) in BODY.iter().take(LEN).enumerate() {
            let c = if i == 0 { 0x39D353 } else { 0x2EA043 };
            fill_rect(
                OX + bx as i32 * CELL + 1,
                OY + by as i32 * CELL + 1,
                CELL - 2,
                CELL - 2,
                c,
            );
        }
        if !ALIVE {
            draw_text(
                OX + 40,
                OY + ROWS * CELL / 2 - 12,
                OVER.as_ptr(),
                OVER.len() as i32,
                0xFFFFFF,
                3,
            );
        }
    }
}
