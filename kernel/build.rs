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

    // 2) GUI demo: imports the drawing ABI (host.fill_rect / host.draw_text) and
    //    paints a titled panel from `render`. Proves a guest can draw real pixels
    //    to the framebuffer through host syscalls — the basis for windowed apps.
    let title = "WASM app  -  desenhado via ABI host";
    let gui = format!(
        r#"(module
  (import "host" "fill_rect" (func $rect (param i32 i32 i32 i32 i32)))
  (import "host" "draw_text" (func $text (param i32 i32 i32 i32 i32 i32)))
  (memory (export "mem") 1)
  (data (i32.const 0) "{title}")
  (func (export "render")
    (call $rect (i32.const 48) (i32.const 56) (i32.const 420) (i32.const 132) (i32.const 0x1E2A4A))
    (call $rect (i32.const 48) (i32.const 56) (i32.const 420) (i32.const 30) (i32.const 0x2D6CDF))
    (call $text (i32.const 60) (i32.const 63) (i32.const 0) (i32.const {len}) (i32.const 0xFFFFFF) (i32.const 2))
    (call $rect (i32.const 64) (i32.const 110) (i32.const 96) (i32.const 56) (i32.const 0xE54B4B))
    (call $rect (i32.const 176) (i32.const 110) (i32.const 96) (i32.const 56) (i32.const 0x3FB57F))
    (call $rect (i32.const 288) (i32.const 110) (i32.const 96) (i32.const 56) (i32.const 0xE8B23A))))"#,
        title = title,
        len = title.len(),
    );
    emit(&out, "gui.wasm", &gui);

    println!("cargo:rerun-if-changed=build.rs");
}

/// Assemble `wat` text and write the resulting binary module to `out/name`.
fn emit(out: &std::path::Path, name: &str, wat: &str) {
    let wasm = wat::parse_str(wat).unwrap_or_else(|e| panic!("{name} WAT failed: {e}"));
    std::fs::write(out.join(name), &wasm).unwrap_or_else(|e| panic!("write {name}: {e}"));
}
