//! Build script for the kernel's embedded WebAssembly programs.
//!
//! Two sources, both turned into binary `.wasm` the kernel embeds with
//! `include_bytes!`:
//!   * `demo.wasm` — a tiny console smoke-test, assembled from inline WAT on the
//!     host (the `wat` crate), so the bare-metal kernel needs no text parser.
//!   * `app.wasm`  — the windowed desktop app, a real Rust crate compiled to
//!     `wasm32-unknown-unknown` (see `../wasm-apps/plasma`). This is the
//!     "compile source to wasm and equip the OS" model: a genuine compiled
//!     language becomes a native OSjeff app, no foreign OS, no emulation.

use std::path::{Path, PathBuf};
use std::process::Command;

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
    emit_wat(&out, "demo.wasm", &console);

    // 2) Windowed app. By default the `snake` game (Rust→wasm) — reproducible on
    //    any machine with the wasm32 target. If `WASI_SDK_PATH` is set, instead
    //    compile a C program to wasm with that SDK's clang: the same C→wasm path a
    //    ported C game (DOOM) rides on. Either way the result is `app.wasm`.
    println!("cargo:rerun-if-env-changed=WASI_SDK_PATH");
    match std::env::var("WASI_SDK_PATH") {
        Ok(sdk) if !sdk.is_empty() => build_c_app(&out, "cdemo", &sdk),
        _ => build_wasm_app(&out, "snake"),
    }

    println!("cargo:rerun-if-changed=build.rs");
}

/// Assemble `wat` text and write the resulting binary module to `out/name`.
fn emit_wat(out: &Path, name: &str, wat: &str) {
    let wasm = wat::parse_str(wat).unwrap_or_else(|e| panic!("{name} WAT failed: {e}"));
    std::fs::write(out.join(name), &wasm).unwrap_or_else(|e| panic!("write {name}: {e}"));
}

/// Compile the workspace-detached `../wasm-apps/<crate>` to
/// `wasm32-unknown-unknown` (release) and copy the resulting module to
/// `out/app.wasm`. Uses a dedicated target dir so the nested build never
/// contends with the outer one's lock.
fn build_wasm_app(out: &Path, crate_name: &str) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let app_dir = root.join("..").join("wasm-apps").join(crate_name);
    let manifest = app_dir.join("Cargo.toml");
    let target_dir = out.join("wasmapp");

    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let status = Command::new(cargo)
        // Build the app as a plain release, isolated from how the kernel itself
        // is being compiled. Without this, running `cargo clippy` on the kernel
        // leaks its clippy wrapper + `-D warnings` into this nested build and the
        // app's own lints would fail the parent lint.
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .env_remove("RUSTC_WRAPPER")
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("CARGO_BUILD_RUSTFLAGS")
        .args(["build", "--release", "--target", "wasm32-unknown-unknown"])
        .arg("--manifest-path")
        .arg(&manifest)
        .arg("--target-dir")
        .arg(&target_dir)
        .status()
        .unwrap_or_else(|e| panic!("failed to launch cargo for {crate_name}: {e}"));
    assert!(status.success(), "wasm app `{crate_name}` failed to build");

    let wasm = target_dir
        .join("wasm32-unknown-unknown/release")
        .join(format!("{crate_name}.wasm"));
    std::fs::copy(&wasm, out.join("app.wasm"))
        .unwrap_or_else(|e| panic!("copy {}: {e}", wasm.display()));

    println!("cargo:rerun-if-changed={}", app_dir.join("src/lib.rs").display());
    println!("cargo:rerun-if-changed={}", manifest.display());
}

/// Compile a freestanding C app `../wasm-apps/<name>/<name>.c` to
/// `wasm32-unknown-unknown` with the wasi-sdk clang at `wasi_sdk`, exporting our
/// app entry points and importing the host ABI. Writes `out/app.wasm`. This is
/// the same toolchain path a ported C game uses.
fn build_c_app(out: &Path, name: &str, wasi_sdk: &str) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let src = root
        .join("..")
        .join("wasm-apps")
        .join(name)
        .join(format!("{name}.c"));
    let clang = PathBuf::from(wasi_sdk).join("bin/clang");
    let app = out.join("app.wasm");

    let status = Command::new(&clang)
        .args([
            "--target=wasm32-unknown-unknown",
            "-O2",
            "-nostdlib",
            "-Wl,--no-entry",
            "-Wl,--export=render",
            "-Wl,--export=on_key",
            "-Wl,--export-memory",
            "-Wl,--allow-undefined",
            "-o",
        ])
        .arg(&app)
        .arg(&src)
        .status()
        .unwrap_or_else(|e| panic!("failed to launch clang ({}): {e}", clang.display()));
    assert!(status.success(), "C app `{name}` failed to build");

    println!("cargo:rerun-if-changed={}", src.display());
}
