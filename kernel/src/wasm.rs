//! Native WebAssembly app engine.
//!
//! WebAssembly is OSjeff's native application format: portable programs compiled
//! to `.wasm` run *inside* the OS through this interpreter ([`wasmi`]) — no
//! foreign OS, no binary emulation, and sandboxed by construction (a guest can
//! only touch its own linear memory and the host functions we explicitly grant).
//!
//! The host surface a guest may import is the OS ABI. For now it is a single
//! `host.log(ptr, len)` syscall that writes a UTF-8 string from guest memory to
//! the serial console; the graphics/input/time syscalls come next (M2).

use crate::serial_print;
use wasmi::{Caller, Engine, Extern, Linker, Module, Store};

/// The demo guest, assembled from WAT at build time (see `build.rs`).
static DEMO_WASM: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/demo.wasm"));

/// Per-instance host state. Empty for now; the ABI grows it (framebuffer handle,
/// window id, allocator cursor) as syscalls are added.
struct HostState;

/// Run the embedded demo module: decode it, wire up the host ABI, and call its
/// exported `main`. Logs each step to serial so the path is observable in QEMU.
pub fn run_demo() {
    match run(DEMO_WASM) {
        Ok(()) => crate::serial_println!("wasm: demo module ran OK"),
        Err(e) => crate::serial_println!("wasm: demo failed: {}", e),
    }
}

/// Instantiate `bytes` as a WebAssembly module, granting it the OS ABI, and
/// invoke its `main` export. Returns a short error string on any failure.
fn run(bytes: &[u8]) -> Result<(), &'static str> {
    let engine = Engine::default();
    let module = Module::new(&engine, bytes).map_err(|_| "module decode")?;

    let mut store = Store::new(&engine, HostState);
    let mut linker = <Linker<HostState>>::new(&engine);

    // host.log(ptr, len): print a UTF-8 string from the guest's linear memory.
    linker
        .func_wrap(
            "host",
            "log",
            |caller: Caller<'_, HostState>, ptr: i32, len: i32| {
                let Some(Extern::Memory(mem)) = caller.get_export("mem") else {
                    return;
                };
                let data = mem.data(&caller);
                let (ptr, len) = (ptr as usize, len as usize);
                if let Some(bytes) = data.get(ptr..ptr.saturating_add(len))
                    && let Ok(s) = core::str::from_utf8(bytes)
                {
                    serial_print!("{}", s);
                }
            },
        )
        .map_err(|_| "link host.log")?;

    let instance = linker
        .instantiate_and_start(&mut store, &module)
        .map_err(|_| "instantiate")?;

    let main = instance
        .get_typed_func::<(), ()>(&store, "main")
        .map_err(|_| "no main export")?;

    serial_print!("wasm: ");
    main.call(&mut store, ()).map_err(|_| "trap in main")?;
    serial_print!("\n");
    Ok(())
}
