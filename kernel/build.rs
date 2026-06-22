//! Build script: assembles the demo WebAssembly modules from inline WAT to
//! binary `.wasm` that the kernel embeds with `include_bytes!`.
//!
//! WAT→wasm assembly runs on the host (where `std` and the `wat` crate are
//! available), so the bare-metal kernel never needs a text-format parser. These
//! are the first programs the OS runs through its native WebAssembly app engine
//! (`kernel/src/wasm.rs`).

use std::path::PathBuf;

fn main() {
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());

    // 1) Console demo: imports host.log(ptr,len), stores a greeting in its own
    //    linear memory, and logs it from `main`. Exercises module decode, memory
    //    export, host import resolution, and a guest→host call.
    let msg = "Hello from WASM - .wasm running native on OSjeff";
    let console = format!(
        r#"(module
  (import "host" "log" (func $log (param i32 i32)))
  (memory (export "mem") 1)
  (data (i32.const 0) "{msg}")
  (func (export "main")
    (call $log (i32.const 0) (i32.const {len}))))"#,
        msg = msg,
        len = msg.len(),
    );
    emit(&out, "demo.wasm", &console);

    // 2) Windowed app: the guest the desktop runs inside a real window. It draws
    //    in its own (0,0)-based coordinate space; the host translates and clips
    //    every primitive to the window's content box. A header, three feature
    //    cards and a footer — a small but genuine native WASM application.
    let s0 = "WASM App nativo";
    let s1 = "rodando dentro do OSjeff - sem emular outro SO";
    let c1 = "wasmi";
    let c2 = "host ABI";
    let c3 = "sandbox";
    let app = format!(
        r#"(module
  (import "host" "fill_rect" (func $r (param i32 i32 i32 i32 i32)))
  (import "host" "draw_text" (func $t (param i32 i32 i32 i32 i32 i32)))
  (memory (export "mem") 1)
  (data (i32.const 0)   "{s0}")
  (data (i32.const 64)  "{s1}")
  (data (i32.const 160) "{c1}")
  (data (i32.const 192) "{c2}")
  (data (i32.const 224) "{c3}")
  (func (export "render")
    (call $r (i32.const 0) (i32.const 0) (i32.const 960) (i32.const 40) (i32.const 0x654FF0))
    (call $t (i32.const 14) (i32.const 9) (i32.const 0) (i32.const {l0}) (i32.const 0xFFFFFF) (i32.const 2))
    (call $r (i32.const 16) (i32.const 60) (i32.const 210) (i32.const 110) (i32.const 0x1E2A4A))
    (call $r (i32.const 240) (i32.const 60) (i32.const 210) (i32.const 110) (i32.const 0x1E2A4A))
    (call $r (i32.const 464) (i32.const 60) (i32.const 210) (i32.const 110) (i32.const 0x1E2A4A))
    (call $r (i32.const 16) (i32.const 60) (i32.const 210) (i32.const 6) (i32.const 0x2D6CDF))
    (call $r (i32.const 240) (i32.const 60) (i32.const 210) (i32.const 6) (i32.const 0x3FB57F))
    (call $r (i32.const 464) (i32.const 60) (i32.const 210) (i32.const 6) (i32.const 0xE8B23A))
    (call $t (i32.const 32) (i32.const 96) (i32.const 160) (i32.const {lc1}) (i32.const 0xE2E8F0) (i32.const 2))
    (call $t (i32.const 256) (i32.const 96) (i32.const 192) (i32.const {lc2}) (i32.const 0xE2E8F0) (i32.const 2))
    (call $t (i32.const 480) (i32.const 96) (i32.const 224) (i32.const {lc3}) (i32.const 0xE2E8F0) (i32.const 2))
    (call $t (i32.const 16) (i32.const 196) (i32.const 64) (i32.const {l1}) (i32.const 0x94A3B8) (i32.const 2))))"#,
        s0 = s0,
        s1 = s1,
        c1 = c1,
        c2 = c2,
        c3 = c3,
        l0 = s0.len(),
        l1 = s1.len(),
        lc1 = c1.len(),
        lc2 = c2.len(),
        lc3 = c3.len(),
    );
    emit(&out, "app.wasm", &app);

    println!("cargo:rerun-if-changed=build.rs");
}

/// Assemble `wat` text and write the resulting binary module to `out/name`.
fn emit(out: &std::path::Path, name: &str, wat: &str) {
    let wasm = wat::parse_str(wat).unwrap_or_else(|e| panic!("{name} WAT failed: {e}"));
    std::fs::write(out.join(name), &wasm).unwrap_or_else(|e| panic!("write {name}: {e}"));
}
