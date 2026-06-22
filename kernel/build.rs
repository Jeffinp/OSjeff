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

    // 2) Windowed app: the interactive guest the desktop runs inside a real
    //    window. It draws in its own (0,0)-based space (host translates + clips
    //    to the content box) and keeps state in its own linear memory + globals.
    //    Click a card (or press Space/Enter) and its counter bar grows — proving
    //    input events (mouse + keyboard) flow host→guest, state persists across
    //    frames, and `host.time_ms` drives a live uptime bar.
    let s0 = "WASM App interativo";
    let s1 = "uptime ao vivo via host.time_ms - clique ou tecle Espaco";
    let c0 = "contador A";
    let c1 = "contador B";
    let c2 = "contador C";
    let app = format!(
        r#"(module
  (import "host" "fill_rect" (func $rect (param i32 i32 i32 i32 i32)))
  (import "host" "draw_text" (func $text (param i32 i32 i32 i32 i32 i32)))
  (import "host" "time_ms" (func $now (result i64)))
  (memory (export "mem") 1)
  (global $sel (mut i32) (i32.const 0))
  (data (i32.const 0)   "{s0}")
  (data (i32.const 64)  "{s1}")
  (data (i32.const 160) "{c0}")
  (data (i32.const 192) "{c1}")
  (data (i32.const 224) "{c2}")
  (func $bump (param $i i32)
    (local $addr i32) (local $v i32)
    (local.set $addr (i32.add (i32.const 256) (i32.mul (local.get $i) (i32.const 4))))
    (local.set $v (i32.add (i32.load (local.get $addr)) (i32.const 14)))
    (if (i32.gt_s (local.get $v) (i32.const 186)) (then (local.set $v (i32.const 186))))
    (i32.store (local.get $addr) (local.get $v)))
  (func $btn (param $idx i32) (param $x i32) (param $accent i32) (param $lp i32) (param $ll i32)
    (local $bg i32) (local $cnt i32)
    (local.set $bg (if (result i32) (i32.eq (local.get $idx) (global.get $sel))
      (then (i32.const 0x26365E)) (else (i32.const 0x1E2A4A))))
    (call $rect (local.get $x) (i32.const 60) (i32.const 210) (i32.const 110) (local.get $bg))
    (call $rect (local.get $x) (i32.const 60) (i32.const 210) (i32.const 6) (local.get $accent))
    (call $text (i32.add (local.get $x) (i32.const 16)) (i32.const 82) (local.get $lp) (local.get $ll) (i32.const 0xE2E8F0) (i32.const 2))
    (local.set $cnt (i32.load (i32.add (i32.const 256) (i32.mul (local.get $idx) (i32.const 4)))))
    (call $rect (i32.add (local.get $x) (i32.const 16)) (i32.const 132) (local.get $cnt) (i32.const 12) (local.get $accent)))
  (func (export "render")
    (local $t i32)
    (call $rect (i32.const 0) (i32.const 0) (i32.const 960) (i32.const 40) (i32.const 0x654FF0))
    (call $text (i32.const 14) (i32.const 9) (i32.const 0) (i32.const {l0}) (i32.const 0xFFFFFF) (i32.const 2))
    (call $btn (i32.const 0) (i32.const 16) (i32.const 0x2D6CDF) (i32.const 160) (i32.const {lc0}))
    (call $btn (i32.const 1) (i32.const 240) (i32.const 0x3FB57F) (i32.const 192) (i32.const {lc1}))
    (call $btn (i32.const 2) (i32.const 464) (i32.const 0xE8B23A) (i32.const 224) (i32.const {lc2}))
    (call $text (i32.const 16) (i32.const 360) (i32.const 64) (i32.const {l1}) (i32.const 0x94A3B8) (i32.const 2))
    (local.set $t (i32.wrap_i64 (i64.rem_u (i64.div_u (call $now) (i64.const 50)) (i64.const 640))))
    (call $rect (i32.const 16) (i32.const 388) (local.get $t) (i32.const 8) (i32.const 0x39507A)))
  (func (export "on_pointer") (param $x i32) (param $y i32) (param $b i32)
    (local $i i32)
    (if (i32.eqz (local.get $b)) (then (return)))
    (if (i32.lt_s (local.get $y) (i32.const 60)) (then (return)))
    (if (i32.ge_s (local.get $y) (i32.const 170)) (then (return)))
    (local.set $i (i32.const -1))
    (if (i32.and (i32.ge_s (local.get $x) (i32.const 16)) (i32.lt_s (local.get $x) (i32.const 226))) (then (local.set $i (i32.const 0))))
    (if (i32.and (i32.ge_s (local.get $x) (i32.const 240)) (i32.lt_s (local.get $x) (i32.const 450))) (then (local.set $i (i32.const 1))))
    (if (i32.and (i32.ge_s (local.get $x) (i32.const 464)) (i32.lt_s (local.get $x) (i32.const 674))) (then (local.set $i (i32.const 2))))
    (if (i32.lt_s (local.get $i) (i32.const 0)) (then (return)))
    (global.set $sel (local.get $i))
    (call $bump (local.get $i)))
  (func (export "on_key") (param $code i32)
    (if (i32.or (i32.eq (local.get $code) (i32.const 32)) (i32.eq (local.get $code) (i32.const 10)))
      (then (call $bump (global.get $sel))))))"#,
        s0 = s0,
        s1 = s1,
        c0 = c0,
        c1 = c1,
        c2 = c2,
        l0 = s0.len(),
        l1 = s1.len(),
        lc0 = c0.len(),
        lc1 = c1.len(),
        lc2 = c2.len(),
    );
    emit(&out, "app.wasm", &app);

    println!("cargo:rerun-if-changed=build.rs");
}

/// Assemble `wat` text and write the resulting binary module to `out/name`.
fn emit(out: &std::path::Path, name: &str, wat: &str) {
    let wasm = wat::parse_str(wat).unwrap_or_else(|e| panic!("{name} WAT failed: {e}"));
    std::fs::write(out.join(name), &wasm).unwrap_or_else(|e| panic!("write {name}: {e}"));
}
