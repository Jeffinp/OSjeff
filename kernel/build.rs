//! Build script: compiles the demo WebAssembly module from inline WAT to a
//! binary `.wasm` that the kernel embeds with `include_bytes!`.
//!
//! WAT→wasm assembly runs on the host (where `std` and the `wat` crate are
//! available), so the bare-metal kernel never needs a text-format parser. The
//! resulting `demo.wasm` is the first program the OS runs through its native
//! WebAssembly app engine (`kernel/src/wasm.rs`).

use std::path::PathBuf;

fn main() {
    // A minimal guest program: it imports the host's `log(ptr, len)` syscall,
    // stores a greeting in its own linear memory, and calls `log` from `main`.
    // This exercises the whole path — module decode, memory export, host import
    // resolution, and a guest→host call — with the smallest possible surface.
    let msg = "Hello from WASM - .wasm running native on OSjeff";
    let wat = format!(
        r#"(module
  (import "host" "log" (func $log (param i32 i32)))
  (memory (export "mem") 1)
  (data (i32.const 0) "{msg}")
  (func (export "main")
    (call $log (i32.const 0) (i32.const {len}))))"#,
        msg = msg,
        len = msg.len(),
    );

    let wasm = wat::parse_str(&wat).expect("demo WAT failed to assemble");

    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("demo.wasm");
    std::fs::write(&out, &wasm).expect("failed to write demo.wasm");

    println!("cargo:rerun-if-changed=build.rs");
}
