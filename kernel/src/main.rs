#![no_std]
#![no_main]
#![warn(clippy::undocumented_unsafe_blocks)]
#![feature(abi_x86_interrupt)]

extern crate alloc;

mod allocator;
mod ata;
mod boot;
mod clock;
mod crash;
mod desktop;
mod fb;
mod fetch;
mod font;
mod gdt;
mod glyphs;
mod icons;
mod interrupts;
mod io;
mod klog;
mod logd;
mod ne2000;
mod netd;
mod netstack;
mod nic;
mod notify;
mod pci;
mod perf;
mod power;
mod ps2;
mod rng;
mod rtc;
mod sched;
mod serial;
mod settings;
mod storage;
mod sync;
mod sysinfo;
mod text;
mod theme;
mod tlsv;
mod trace;
mod virtio;
mod virtio_gpu;
mod virtio_net;
mod virtio_rng;
mod vm;
mod wasm;

use bootloader_api::config::{BootloaderConfig, Mapping};
use bootloader_api::info::FrameBufferInfo;
use bootloader_api::{BootInfo, entry_point};
use core::panic::PanicInfo;
use desktop::{CURSOR_H, CURSOR_W, Desktop};
use fb::Canvas;
use osjeff_core::cursor::CursorTrack;
use osjeff_core::{Rect, Time};
use ps2::Event;

// Ask the bootloader to map all physical memory at a fixed offset. This gives
// the kernel a `physical_memory_offset` so it can translate between virtual and
// physical addresses — required for DMA: the virtio-gpu device reads its
// descriptor rings and buffers by physical address.
static BOOT_CONFIG: BootloaderConfig = {
    let mut c = BootloaderConfig::new_default();
    c.mappings.physical_memory = Some(Mapping::Dynamic);
    // The default 80 KiB boot stack (which the compositor thread runs on) has
    // only ~11 KiB in use but no canary; the recursive HTML/CSS layout and the
    // TLS stack are the deepest users. 512 KiB keeps a wide margin; the
    // bootloader still adds a guard page below it.
    c.kernel_stack_size = 512 * 1024;
    c
};

entry_point!(kernel_main, config = &BOOT_CONFIG);

// Render buffers sized for up to 1920x1080x4. BACK is the compositing target;
// BG caches the static wallpaper so it is never recomputed per frame.
const MAX_BYTES: usize = 1920 * 1080 * 4;

// 64-byte aligned so the framebuffer fast paths can reinterpret rows as `[u32]`
// (one 32-bit store per pixel, vectorized) without an unaligned cast.
#[repr(C, align(64))]
struct AlignedBuf([u8; MAX_BYTES]);

use sync::RacyCell;

static BACK: RacyCell<AlignedBuf> = RacyCell::new(AlignedBuf([0; MAX_BYTES]));
static BG: RacyCell<AlignedBuf> = RacyCell::new(AlignedBuf([0; MAX_BYTES]));
// Cached "everything except the animating window(s)" layer, composed once per
// animation so each frame only redraws the small damaged region.
static STATIC: RacyCell<AlignedBuf> = RacyCell::new(AlignedBuf([0; MAX_BYTES]));

// Kernel heap backing the global allocator. Sized for the heaviest user: a
// native WASM app's linear memory (DOOM grows its wasm memory to ~16-20 MiB for
// its zone + the IWAD it reads in), well above what the TLS 1.3 handshake or the
// smoltcp socket buffers need. Zero-initialized BSS — no on-disk image cost.
const HEAP_SIZE: usize = 64 * 1024 * 1024;
static HEAP: RacyCell<[u8; HEAP_SIZE]> = RacyCell::new([0; HEAP_SIZE]);

#[global_allocator]
static ALLOCATOR: allocator::LockedHeap = allocator::LockedHeap::new();

fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    // Serial first: a text log on COM1 that survives even if the framebuffer is
    // missing, so driver bring-up is observable without the screen.
    serial::init();
    trace::mark("kernel entry");
    klog!(Info, "OSjeff boot: kernel entry");

    // Capture the physical-memory offset before `boot_info` is borrowed for the
    // framebuffer. Needed for DMA (virtio-gpu addresses memory physically).
    let phys_offset = boot_info.physical_memory_offset.into_option();
    let kernel_len = boot_info.kernel_len;
    klog!(Info, "physical_memory_offset: {:#x?}", phys_offset);
    // The page tables are reached through that mapping (thread-stack guard pages, see `vm`).
    vm::init(phys_offset);

    let framebuffer = match boot_info.framebuffer.as_mut() {
        Some(fb) => fb,
        None => {
            serial_println!("FATAL: bootloader provided no framebuffer");
            crash::halt()
        }
    };
    let info = framebuffer.info();
    sysinfo::capture(
        &boot_info.memory_regions,
        kernel_len,
        info.width,
        info.height,
    );
    // From here on a panic or fatal exception paints an error page on this framebuffer.
    crash::register_framebuffer(framebuffer.buffer_mut(), info);
    let n = framebuffer.buffer().len().min(MAX_BYTES);

    // The render buffers are fixed-size statics and `Canvas` indexes them with the real
    // layout, so a larger screen would panic with no message once drawing starts. Refuse
    // cleanly instead, saying what was detected.
    let fb_need = info.stride * info.height * info.bytes_per_pixel;
    if fb_need > n {
        crash::die(
            crash::Kind::Unsupported,
            "this screen resolution is not supported",
            format_args!(
                "Detected {}x{} (stride {}, {} bytes/pixel): the screen needs {} bytes, the \
                 bootloader provided a framebuffer of {} bytes and the kernel's render \
                 buffers hold at most {} bytes (1920x1080x4).\nSelect a smaller resolution \
                 in the firmware or bootloader settings (1920x1080 or lower) and reboot.",
                info.width,
                info.height,
                info.stride,
                info.bytes_per_pixel,
                fb_need,
                framebuffer.buffer().len(),
                MAX_BYTES,
            ),
            None,
        );
    }

    // Wipe the bootloader's on-screen debug log immediately, so the early init
    // (PCI scan, TSC calibration, DHCP) shows a clean screen instead of a frozen
    // wall of text until the splash takes over.
    framebuffer.buffer_mut()[..n].fill(0);
    trace::mark("framebuffer cleared");

    // SAFETY: BACK/BG/STATIC are three distinct statics, each viewed once here (`kernel_main` runs
    // once, on the boot thread) with `n <= MAX_BYTES`, their size; afterwards only the compositor
    // loop uses them, so each `&mut [u8]` is unique.
    let back: &mut [u8] = unsafe { core::slice::from_raw_parts_mut(BACK.get() as *mut u8, n) };
    // SAFETY: as for `back` (BG is a separate static).
    let bg: &mut [u8] = unsafe { core::slice::from_raw_parts_mut(BG.get() as *mut u8, n) };
    // SAFETY: as for `back` (STATIC is a separate static).
    let static_buf: &mut [u8] =
        unsafe { core::slice::from_raw_parts_mut(STATIC.get() as *mut u8, n) };

    // Initialize the kernel heap so `alloc` works, then smoke-test it.
    // SAFETY: HEAP is a dedicated 64 MiB static, valid and writable, used for nothing else; this
    // runs once, on the boot thread, before the first allocation.
    // NOTE: HEAP's type has align 1; the 8-byte alignment `init` needs comes from linker placement
    // and is only `debug_assert!`ed.
    unsafe {
        ALLOCATOR.init(HEAP.get() as usize, HEAP_SIZE);
    }
    heap_smoke_test();
    trace::mark("heap init + smoke test");

    // First run through the native WebAssembly app engine: prove the OS can load
    // and execute a `.wasm` program (its native app format) end to end. Output
    // lands on serial. The graphics/input ABI (windowed apps) builds on this.
    klog!(Info, "OSjeff boot: running native WASM demo");
    wasm::run_demo();
    trace::mark("wasm demo done");

    // Enumerate the PCI bus — groundwork for the virtio-gpu driver: locate the
    // device and, when present, enable bus mastering so a later DMA-capable
    // driver can use it. QEMU captures the log via `-serial file:...`.
    klog!(Info, "OSjeff boot: enumerating PCI bus 0");
    pci::for_each(|d| {
        klog!(
            Info,
            "  pci {:02x}:{:02x}.{}  {:04x}:{:04x}",
            d.bus,
            d.slot,
            d.func,
            d.vendor,
            d.device
        );
    });
    match pci::find_virtio_gpu() {
        Some(gpu) => {
            gpu.enable_bus_master();
            klog!(
                Info,
                "virtio-gpu @ slot {} func {} bar0={:#010x}",
                gpu.slot,
                gpu.func,
                gpu.bar(0)
            );
            match virtio::discover(&gpu) {
                Some(caps) => {
                    klog!(
                        Info,
                        "  common bar{} off={:#x} len={}",
                        caps.common.bar,
                        caps.common.offset,
                        caps.common.length
                    );
                    klog!(
                        Info,
                        "  notify bar{} off={:#x} mul={}",
                        caps.notify.bar,
                        caps.notify.offset,
                        caps.notify_off_mul
                    );
                    klog!(
                        Info,
                        "  isr    bar{} off={:#x}   device bar{} off={:#x}",
                        caps.isr.bar,
                        caps.isr.offset,
                        caps.device.bar,
                        caps.device.offset
                    );
                }
                None => klog!(Info, "  virtio caps: none (transitional device?)"),
            }

            // Bring up the full virtio-gpu driver: negotiate, set up the control
            // queue (DMA), flip DRIVER_OK, and query the display geometry — which
            // exercises the entire command path end to end.
            if let (Some(off), Some(caps)) = (phys_offset, virtio::discover(&gpu)) {
                match virtio_gpu::GpuDevice::init(&gpu, caps, off) {
                    Some(mut dev) => {
                        klog!(Info, "virtio-gpu: control queue up, DRIVER_OK");
                        match dev.get_display_info() {
                            Some((w, h)) => {
                                klog!(Info, "virtio-gpu display 0: {}x{}", w, h)
                            }
                            None => klog!(Warn, "virtio-gpu: get_display_info failed"),
                        }
                        // Exercise the 2D command path (no scanout swap — the VBE
                        // display stays live). The accelerated scanout is the next step.
                        dev.verify_2d();
                    }
                    None => klog!(Warn, "virtio-gpu: init failed"),
                }
            }
        }
        None => klog!(Info, "virtio-gpu: absent — using the VBE framebuffer"),
    }
    trace::mark("pci scan + virtio-gpu probe done");

    // Kernel scheduler: register the boot context (the compositor) as thread 0.
    // The preemptive round-robin is in place for real future threads, but we no
    // longer spawn demo spin-workers — they consumed 2/3 of the CPU under the
    // equal-slice round-robin, starving the compositor and capping the frame
    // rate. With the GUI as the sole thread it gets the whole core.
    // Probe the IDE channels: report where the OS is installed (boot disk) and
    // whether each disk is a spinning HD or an SSD, so storage can adapt.
    ata::detect_and_log();
    trace::mark("ata detect done");

    // Own GDT + TSS (IST stack for #DF) before anything captures CS/SS.
    gdt::init();

    sched::init();

    // Configure the PS/2 controller BEFORE enabling interrupts, so the init
    // handshake (config write + mouse ACKs) is read by polling without racing
    // the IRQ handlers.
    ps2::init();

    // Real interrupts: IDT + exception handlers, PIC remap, PIT timer, and
    // IRQ-driven keyboard (IRQ1) + mouse (IRQ12).
    interrupts::init();
    trace::mark("sched + ps2 + idt/pic/pit init");

    // Calibrate the TSC against the now-running PIT so the perf HUD can report
    // frame time in real milliseconds.
    let tsc_khz = perf::calibrate_khz();
    klog!(Info, "TSC calibrated: {} kHz", tsc_khz);
    netd::set_tsc_khz(tsc_khz);
    // Entropy before the first consumer (DHCP transaction ids, TCP sequence numbers): probes
    // RDSEED/RDRAND and virtio-rng, and starts crediting the timer-interrupt samples.
    rng::init(phys_offset);
    trace::mark("tsc calibrated (25 PIT ticks)");
    trace::calibrated(tsc_khz);
    trace::bench_prims(back, info, tsc_khz);
    trace::bench_alloc(tsc_khz);
    let mut perf = perf::Perf::new(tsc_khz);

    // Bring up the NIC (if present), lease an IP over DHCP (falling back to the
    // static address if no server answers), and announce ourselves with a
    // gratuitous ARP so the network is visible on the wire from boot. No card ->
    // skip and keep the static IP. The `Port` is then moved into the network stack
    // that the `fetcher` thread owns: from there on it is the only party that can
    // touch the hardware.
    let port = nic::probe(phys_offset);
    trace::mark("nic init done");
    // `Netd::boot` runs the DHCP exchange (falling back to the static address) and builds the stack
    // around the port. The result is handed to the `fetcher` thread, which is from then on the only
    // party that can touch the hardware (DHCP renewal, ARP/ping and fetches all run there).
    let netd = port.map(netd::Netd::boot);
    trace::mark("dhcp done");
    // Capture the RTC (UTC) once, before any other thread can touch the CMOS
    // ports: it is the local clock that SNTP then corrects for TLS date checks.
    clock::init();

    // Hand the browser's TCP/IP + TLS stack to the background fetcher and spawn
    // its worker thread, so page loads (and the slow software TLS handshake) run
    // off the compositor thread and never freeze the UI. Interrupts are disabled
    // around `spawn` because it mutates the scheduler's thread list, which the
    // timer ISR also reads.
    match netd {
        Some(netd) => {
            fetch::init(netd);
            x86_64::instructions::interrupts::without_interrupts(|| {
                sched::spawn("fetcher", fetch::worker);
            });
        }
        None => fetch::init_offline(),
    }

    // Hand the framebuffer layout to the WASM app manager and spawn `appd`, the
    // thread that runs every app (round-robin), so a heavy app (DOOM loading its
    // WAD, then running its game loop) renders off the compositor thread and never
    // freezes the UI.
    wasm::init(info, tsc_khz);
    x86_64::instructions::interrupts::without_interrupts(|| {
        sched::spawn("appd", wasm::worker);
        // The terminal's command thread: sleep, ping and curl wait here, not in the compositor.
        sched::spawn("shelld", desktop::shell_worker);
        sched::spawn("shelld2", desktop::shell_worker2);
    });

    trace::mark("threads spawned (fetcher, appd, shelld x2)");

    // Storage service: detect/mount/migrate OJFS v3 on the filesystem disk (the desktop
    // still runs on the v2 image; see storage.rs). Runs with the scheduler up, so the
    // ATA driver can yield to the other threads between sectors.
    storage::init();
    trace::mark("storage init done");

    // UI text engine: parse the embedded fonts and rasterise the faces the chrome
    // draws on every frame (the rest of the glyph cache fills on first use).
    {
        let (us, st) = text::init(tsc_khz);
        klog!(
            Info,
            "ui text: {} glyphs, {} KiB atlas in {} us",
            st.glyphs,
            st.arena_bytes / 1024,
            us
        );
        if trace::ON {
            serial_println!(
                "[trace] ui: text atlas {} glyphs {} bytes {} strips built in {} us",
                st.glyphs,
                st.arena_bytes,
                st.strips,
                us
            );
        }
    }
    trace::mark("ui text engine ready");
    // logd: persists the system log off the compositor thread (the boot log is written
    // after the first desktop frame, see `logd`).
    x86_64::instructions::interrupts::without_interrupts(|| {
        sched::spawn("logd", logd::worker);
    });

    // Boot splash: progress tracks real elapsed time (>= 5 seconds).
    run_splash(&mut *framebuffer, &mut *back, info, n);
    trace::mark("splash end (artificial >= 5 s)");

    // The desktop (and its filesystem, which holds the stored settings) comes first so
    // the wallpaper and accent can honour them; with no settings file the defaults
    // paint exactly what the desktop always looked like.
    let mut desk = Desktop::new(info.width as i32, info.height as i32);
    trace::mark("Desktop::new (fs load from ATA) done");
    desk.load_settings();

    // Static layer painted once.
    {
        let mut c = Canvas::new(&mut *bg, info);
        desktop::paint_background(&mut c);
    }
    trace::mark("wallpaper painted");
    let mut last_sec = 0xFFu8; // force first render
    // Where the cursor sprite is painted in the framebuffer (see `osjeff_core::cursor`: erased at
    // the start of every rendering frame, painted again at its end).
    let mut cursor = CursorTrack::new(CURSOR_W, CURSOR_H);
    // Seed from the live tick count, NOT 0: the splash ran for ~5 s with the
    // timer firing, so a 0 seed would make the first frame's delta enormous and
    // instantly complete the open animation (skipping the full-screen blit that
    // clears the splash).
    let mut last_tick = interrupts::ticks();

    // Animation fast-path state. `was_anim` starts true so the first steady
    // frame forces one full repaint over the splash even if no animation runs.
    let mut was_anim = true;
    let mut was_overlay = false;
    let mut static_valid = false;
    let mut last_sig = 0u64;
    let mut prev_damage = Rect::new(0, 0, 0, 0);
    // Focused window rect from the previous steady frame, so a content change can
    // also repaint the window that just lost focus (its title de-highlights).
    let mut prev_focused: Option<Rect> = None;
    let mut last_hud = 0u64; // tick of the last perf-HUD refresh
    let mut hud_was_on = false;
    // Set after a browser fetch completes so the next iteration repaints the
    // page (the fetch itself blocks, so it can't render in its own frame).
    let mut browser_redraw = false;

    // Animations run on real time: seconds per timer tick (250 Hz), independent of how
    // often the GUI thread is scheduled (the timer preempts round-robin across threads).
    const DT_PER_TICK: f32 = 1.0 / interrupts::TIMER_HZ as f32;
    let mut first_frame = true;
    // Screen area the toasts covered at the last repaint (to restore it).
    let mut prev_toast_rect = Rect::new(0, 0, 0, 0);

    loop {
        let rt = rtc::now();
        let time = Time {
            h: rt.h,
            m: rt.m,
            s: rt.s,
        };

        // Advance animation by real elapsed timer ticks.
        let tick = interrupts::ticks();
        let tick_delta = tick.saturating_sub(last_tick);
        last_tick = tick;
        let tick_changed = tick_delta > 0;
        if tick_changed {
            desk.animate(tick_delta as f32 * DT_PER_TICK);
        }

        // Drain all pending PS/2 events.
        let mut scene_dirty = core::mem::take(&mut browser_redraw);
        let mut cursor_moved = false;
        let mut clock_tick = false;
        let mut got_input = false;
        trace::loop_iter();
        while let Some(event) = ps2::poll() {
            got_input = true;
            match event {
                Event::Mouse(p) => {
                    let r = desk.handle_mouse(p.dx, p.dy, p.left, p.right);
                    scene_dirty |= r.scene_dirty;
                    cursor_moved |= r.cursor_moved;
                    // The wheel goes to the window under the pointer.
                    if p.dz != 0 && desk.handle_wheel(p.dz) {
                        scene_dirty = true;
                    }
                }
                Event::Key(k) => {
                    if desk.handle_key(k.scan_code, k.extended, k.pressed, time) {
                        scene_dirty = true;
                    }
                }
            }
        }

        let input_irq_tsc = if got_input { trace::input_taken() } else { 0 };
        let mut report_due = false;
        if rt.s != last_sec {
            last_sec = rt.s;
            report_due = true;
            desk.tick_processes();
            clock_tick = true;
            perf.second_tick();
            let (fps, frame_us, max_us) = perf.stats();
            desk.sample_system(&desktop::SysInputs {
                ticks: interrupts::ticks(),
                busy: sched::busy_ticks(),
                heap_used: HEAP_SIZE - ALLOCATOR.free_bytes().min(HEAP_SIZE),
                heap_total: HEAP_SIZE,
                fps,
                frame_us,
                max_us,
                tsc_khz,
            });
        }

        // Tell the app manager each WASM window's size / visibility; reap finished apps.
        desk.wasm_sync();

        // The wallpaper or accent changed: repaint the cached background and
        // recompose everything (the settings app also asked for a full repaint).
        if desk.take_bg_repaint() {
            let mut c = Canvas::new(&mut *bg, info);
            desktop::paint_background(&mut c);
            static_valid = false;
            scene_dirty = true;
        }

        // A maximize / restore / vanished minimized window changes pixels well
        // outside the focused window: repaint (and upload) the whole screen once.
        // The frame after an overlay (menu, start panel, Alt+Tab) closes must also
        // repaint everything: the overlay covered pixels outside the focused window.
        let overlay_closed = was_overlay && !desk.overlay_open();
        was_overlay = desk.overlay_open();
        let force_full = desk.take_full_repaint() || overlay_closed;
        scene_dirty |= force_full;
        let extra_dirty = desk.take_extra_dirty();

        let any_anim = desk.has_animation();

        // The cheap clock-tick repaint (rect-only blit) is only valid in the
        // steady desktop. While an overlay or animation is up, fold the tick
        // into a normal recompose so those transient layers stay consistent.
        if clock_tick && (any_anim || desk.overlay_open()) {
            scene_dirty = true;
            clock_tick = false;
        }

        // The pointer changed since the sprite was painted: it moved (possibly several PS/2
        // packets since the last frame, clamped at an edge or not), or it turned into a hand
        // over a link without moving. Anything else that renders also repaints the sprite.
        cursor_moved |= cursor.is_stale(desk.pointer());
        // Switching the HUD on or off repaints its corner, and the frame must end with the cursor.
        cursor_moved |= desk.hud_visible() != hud_was_on;

        // Did this iteration do real rendering work? (Used to time frames.)
        let work = any_anim || scene_dirty || clock_tick || cursor_moved || was_anim;
        let frame_start = io::rdtsc();
        let cpu_start = trace::cpu_now();
        let mut path: Option<trace::Path> = None;

        // CURSOR INVARIANT (see `osjeff_core::cursor`): the sprite lives only in the
        // framebuffer, never in `back`. Every frame that renders anything first erases it
        // (restores the old sprite box from `back`), then does its own uploads, toasts and
        // HUD, and paints the sprite again as the very last step. No path restores or draws
        // the cursor on its own, so none can forget to.
        let mut erased_hud = false;
        if work && let Some(r) = cursor.erase(info.width as i32, info.height as i32) {
            fb_blit_rect(framebuffer.buffer_mut(), back, info, r.x, r.y, r.w, r.h, n);
            let hr = perf::Perf::rect(info.width as i32);
            erased_hud = hr.intersection(&r).is_some();
        }

        if any_anim {
            // ---- Animation fast-path: cache the static scene, then each frame
            // only touch the small damaged region around the animating window.
            let sig = desk.anim_signature();
            if !static_valid || sig != last_sig || scene_dirty {
                path = Some(trace::Path::AnimRebuild);
                let tc = trace::t();
                copy_bg(static_buf, bg);
                desk.compose_static(static_buf, info, time);
                back.copy_from_slice(static_buf);
                // Draw the animating window(s) into `back` BEFORE the full blit
                // so the framebuffer never shows a frame with them missing.
                // Without this, closing a *visible* window blanks it for one
                // full-screen blit (a visible blink) before the damage pass
                // redraws it — the static layer excludes animating windows.
                let dmg = desk.render_anim_frame(back, static_buf, info, Rect::new(0, 0, 0, 0));
                trace::stage(trace::Stage::Compose, tc);
                fb_full_blit(framebuffer.buffer_mut(), back, n);
                static_valid = true;
                last_sig = sig;
                prev_damage = dmg;
            }

            if tick_changed || cursor_moved {
                path.get_or_insert(trace::Path::AnimDamage);
                let tc = trace::t();
                let damage = desk.render_anim_frame(back, static_buf, info, prev_damage);
                trace::stage(trace::Stage::Compose, tc);
                fb_blit_rect(
                    framebuffer.buffer_mut(),
                    back,
                    info,
                    damage.x,
                    damage.y,
                    damage.w,
                    damage.h,
                    n,
                );
                prev_damage = damage;
            }
        } else if desk.overlay_open() {
            // ---- Overlay fast-path: a context menu or the start panel is open.
            // Cache the overlay-less scene in STATIC, then repaint only the
            // overlay's rectangle as the cursor moves (hover highlight) — O(menu)
            // instead of recomposing every window + shadow on each mouse move.
            let sig = desk.anim_signature();
            let ov = desk.overlay_bounds();
            if !static_valid || sig != last_sig || scene_dirty {
                path = Some(trace::Path::OverlayRebuild);
                let tc = trace::t();
                copy_bg(static_buf, bg);
                desk.compose_static(static_buf, info, time);
                back.copy_from_slice(static_buf);
                {
                    let mut c = Canvas::new(back, info);
                    desk.draw_overlay(&mut c);
                }
                trace::stage(trace::Stage::Compose, tc);
                fb_full_blit(framebuffer.buffer_mut(), back, n);
                static_valid = true;
                last_sig = sig;
            } else if cursor_moved {
                // Restore the overlay-less scene under the overlay rect, redraw
                // the overlay (updated hover), and blit just that rect.
                path = Some(trace::Path::OverlayHover);
                let tc = trace::t();
                blit_rect(back, static_buf, info, ov.x, ov.y, ov.w, ov.h, n);
                {
                    let mut c = Canvas::new(back, info);
                    desk.draw_overlay_dirty(&mut c);
                }
                trace::stage(trace::Stage::Compose, tc);
                fb_blit_rect(
                    framebuffer.buffer_mut(),
                    back,
                    info,
                    ov.x,
                    ov.y,
                    ov.w,
                    ov.h,
                    n,
                );
            }
        } else {
            static_valid = false;
            // A finished animation needs one final full recompose to settle.
            if was_anim {
                scene_dirty = true;
            }
            let settle = was_anim || force_full;

            if scene_dirty {
                path = Some(if settle {
                    trace::Path::Settle
                } else {
                    trace::Path::Steady
                });
                let tc = trace::t();
                copy_bg(back, bg);
                desk.render(back, info, time);
                trace::stage(trace::Stage::Compose, tc);
                if settle {
                    // Settle frame after an animation (and the first desktop
                    // frame): the whole scene may differ, so blit it all.
                    fb_full_blit(framebuffer.buffer_mut(), back, n);
                } else {
                    // Steady content change (a keystroke, a calc button, a
                    // focus/z-order switch): the only pixels that differ live in
                    // the focused window, the one that just lost focus (its title
                    // de-highlights), and the clock. Upload just those rects
                    // instead of the whole ~8 MiB framebuffer.
                    let mut up = |r: Rect| {
                        fb_blit_rect(framebuffer.buffer_mut(), back, info, r.x, r.y, r.w, r.h, n);
                    };
                    if let Some(fb_) = desk.focused_box() {
                        up(fb_);
                    }
                    if let Some(pf) = prev_focused {
                        up(pf);
                    }
                    // Hover changes repaint the window the pointer entered / left.
                    if let Some(r) = extra_dirty {
                        up(r);
                    }
                    up(desk.clock_rect());
                    up(desk.panel_rect());
                }
            } else if clock_tick {
                let tc = trace::t();
                // Per-second tick, nothing else changed: refresh `back` but upload
                // only the clock pill — plus the Task Manager window if open — to
                // VRAM, skipping the ~8 MiB full-screen blit that made the clock
                // tick hitch every second.
                if desk.clock_repaint_is_local() {
                    // `back` still holds the scene composed by the last full
                    // frame (nothing animates, no overlay) and no window or
                    // shadow reaches the pill: wallpaper + clock is the whole
                    // difference, so redo just that rectangle.
                    path = Some(trace::Path::ClockLocal);
                    desk.repaint_clock(back, bg, info, time);
                } else {
                    path = Some(trace::Path::Clock);
                    copy_bg(back, bg);
                    desk.render(back, info, time);
                }
                trace::stage(trace::Stage::Compose, tc);
                let cr = desk.clock_rect();
                fb_blit_rect(
                    framebuffer.buffer_mut(),
                    back,
                    info,
                    cr.x,
                    cr.y,
                    cr.w,
                    cr.h,
                    n,
                );
                if let Some(tr) = desk.task_window_rect() {
                    fb_blit_rect(
                        framebuffer.buffer_mut(),
                        back,
                        info,
                        tr.x,
                        tr.y,
                        tr.w,
                        tr.h,
                        n,
                    );
                }
            } else if cursor_moved {
                path = Some(trace::Path::Cursor);
            }
            prev_focused = desk.focused_box();
        }
        was_anim = any_anim;

        // Record the frame time (only when we actually rendered).
        if work {
            perf.record(io::rdtsc().wrapping_sub(frame_start));
            if let Some(p) = path {
                trace::frame(p, frame_start, cpu_start);
            }
            trace::input_done(input_irq_tsc);
            if first_frame {
                first_frame = false;
                trace::mark("first desktop frame composed + blitted");
                let ts = text::stats();
                klog::log_quiet(
                    klog::Level::Info,
                    format_args!(
                        "ui memory: glyph atlas {} KiB ({} glyphs), icon cache {} KiB",
                        ts.arena_bytes / 1024,
                        ts.glyphs,
                        icons::bytes() / 1024
                    ),
                );
                if trace::ON {
                    serial_println!(
                        "[trace] ui: memory after the first frame: glyph atlas {} bytes ({} glyphs), icon cache {} bytes",
                        ts.arena_bytes,
                        ts.glyphs,
                        icons::bytes()
                    );
                }
                klog::log_quiet(
                    klog::Level::Info,
                    format_args!("first desktop frame composed + blitted"),
                );
                logd::request_boot_flush();
            }
        }

        // Toast notifications: new events (WARN+ log records, notify() calls) and
        // expiry. Only while a toast is on screen, or has just left, is anything
        // repainted: restore the scene under them from `back`, draw them on top.
        let toast_changed = desk.poll_toasts(klog::ticks_to_ms_now());
        let mut toast_drawn = false;
        if toast_changed || (work && !desk.toasts_idle()) {
            let tt = trace::t();
            let cur = desk.toast_bounds();
            let r = if prev_toast_rect.is_empty() {
                cur
            } else if cur.is_empty() {
                prev_toast_rect
            } else {
                prev_toast_rect.union(&cur)
            };
            blit_rect(framebuffer.buffer_mut(), back, info, r.x, r.y, r.w, r.h, n);
            {
                let mut c = Canvas::new(&mut framebuffer.buffer_mut()[..n], info);
                desk.draw_toasts(&mut c);
            }
            toast_drawn = true;
            prev_toast_rect = cur;
            trace::stage(trace::Stage::Hud, tt);
        }

        // Perf HUD: refresh ~10x/s as a framebuffer overlay restored from `back`,
        // so it never pollutes the cached scene. Drawn outside the timed window.
        // (Also when erasing the old cursor box just wiped part of it.)
        let hud_on = desk.hud_visible();
        let mut hud_drawn = false;
        if hud_on != hud_was_on {
            // Switched on or off: the next refresh draws it, or restores what it covered
            // (the cursor is painted last by the frame's own final step).
            hud_was_on = hud_on;
            last_hud = 0;
            if !hud_on {
                let hr = perf::Perf::rect(info.width as i32);
                fb_blit_rect(
                    framebuffer.buffer_mut(),
                    back,
                    info,
                    hr.x,
                    hr.y,
                    hr.w,
                    hr.h,
                    n,
                );
            }
        }
        if hud_on && (tick.saturating_sub(last_hud) >= 25 || erased_hud) {
            last_hud = tick;
            hud_drawn = true;
            let used = HEAP_SIZE - ALLOCATOR.free_bytes().min(HEAP_SIZE);
            let heap_pct = (used * 100 / HEAP_SIZE) as u32;
            let hr = perf::Perf::rect(info.width as i32);
            let th = trace::t();
            blit_rect(
                framebuffer.buffer_mut(),
                back,
                info,
                hr.x,
                hr.y,
                hr.w,
                hr.h,
                n,
            );
            let mut c = Canvas::new(&mut framebuffer.buffer_mut()[..n], info);
            perf.draw(&mut c, heap_pct, sched::thread_count());
            trace::stage(trace::Stage::Hud, th);
        }

        // Cursor: painted last (step 3 of the invariant above), over the frame, the toasts and
        // the HUD. A toast or HUD restore from `back` can also wipe a sprite that was already
        // painted in a frame that did no rendering, so those repaint it too.
        if work || toast_drawn || hud_drawn {
            cursor_to_fb(&desk, &mut cursor, framebuffer.buffer_mut(), info, n);
        }
        if report_due {
            trace::report(tsc_khz, tick);
        }

        // Browser navigation: hand any pending request to the background fetcher
        // (non-blocking — the worker thread does the slow fetch while we keep
        // rendering the "Carregando" state), and pick up a finished result.
        if fetch::is_idle() {
            let mut url = [0u8; 512];
            if let Some(len) = desk.browser_take_request(&mut url) {
                let mut host = [0u8; 96];
                let hlen = desk.browser_insecure_host(&mut host);
                fetch::try_post(&url[..len], &host[..hlen]);
            } else if let Some((len, fit_w)) = desk.browser_next_image(&mut url) {
                // Nothing else is waiting: fetch the next picture of the page.
                if !fetch::try_post_image(&url[..len], &[], fit_w) {
                    desk.browser_image_done(Err(osjeff_core::web::imgcache::ImgFail::Failed));
                    browser_redraw = true;
                }
            }
        } else if fetch::worker_dead() {
            // The fetcher thread died: fail the navigation now instead of leaving the
            // browser on "Carregando" forever.
            let mut url = [0u8; 512];
            if desk.browser_take_request(&mut url).is_some() {
                desk.browser_fail(osjeff_core::browser::FailReason::WorkerDied);
                browser_redraw = true;
            }
        }
        if desk.browser_poll_internal() {
            browser_redraw = true;
        }
        if let Some(res) = fetch::take_image_result() {
            desk.browser_image_done(res);
            browser_redraw = true;
        }
        if let Some(result) = fetch::take_result() {
            match result {
                Ok(page) => desk.browser_load(&page.data, page.conn, page.truncated, page.cert),
                Err(reason) => desk.browser_fail(reason),
            }
            browser_redraw = true;
        }

        // Idle until the next interrupt instead of busy-spinning. The timer
        // (250 Hz) wakes us to step animations and the per-second clock; the
        // keyboard/mouse IRQs wake us immediately on input. This paces frames to
        // the tick rate and stops the compositor from burning a full core — a
        // real system halts when it has nothing to draw.
        //
        // `sched::idle` does this without the old lost-wakeup window (input that
        // lands between the last poll and the `hlt` is noticed with interrupts
        // masked) and, when a worker thread is runnable, gives it the CPU now
        // rather than halting through the rest of our slice.
        sched::idle(interrupts::input_pending);
        trace::hlt_wake();
    }
}

fn secs_of_day(t: rtc::Time) -> u32 {
    t.h as u32 * 3600 + t.m as u32 * 60 + t.s as u32
}

/// Plays the boot splash, driving the progress bar from real elapsed RTC time
/// so it always lasts at least 5 seconds regardless of CPU speed.
fn run_splash(
    framebuffer: &mut bootloader_api::info::FrameBuffer,
    back: &mut [u8],
    info: FrameBufferInfo,
    n: usize,
) {
    let start = secs_of_day(rtc::now());
    let mut prev_el = 0u32;
    let mut frac = 0.0f32;
    let (mut frames, mut draw_cyc, mut blit_cyc) = (0u64, 0u64, 0u64);
    loop {
        let el = (secs_of_day(rtc::now()) + 86_400 - start) % 86_400;
        if el != prev_el {
            frac = 0.0;
            prev_el = el;
        }
        let p = ((el as f32) + frac.min(0.99)) / 5.0;
        let t0 = trace::t();
        {
            let mut c = Canvas::new(back, info);
            boot::draw_splash(&mut c, p);
        }
        let t1 = trace::t();
        fb_full_blit(framebuffer.buffer_mut(), back, n);
        let t2 = trace::t();
        if trace::ON {
            frames += 1;
            draw_cyc += t1 - t0;
            blit_cyc += t2 - t1;
        }
        io::delay_cycles(20_000_000);
        frac += 0.06;
        if el >= 5 {
            break;
        }
    }
    if trace::ON {
        // Raw TSC cycles (the TSC rate is printed on the "TSC calibrated" line).
        serial_println!(
            "[trace] splash: {} frames, draw avg {} cyc, blit avg {} cyc, delay_cycles 20000000 per frame",
            frames,
            draw_cyc / frames.max(1),
            blit_cyc / frames.max(1)
        );
    }
}

/// Restore the cached wallpaper into a render buffer (timed as `Prim::BgCopy`).
fn copy_bg(dst: &mut [u8], bg: &[u8]) {
    let t0 = trace::t();
    dst.copy_from_slice(bg);
    trace::prim(trace::Prim::BgCopy, t0);
}

/// Draw the cursor straight into the framebuffer (timed as `Stage::Cursor`) and record where it
/// went. Always the last step of a frame, after `CursorTrack::erase` restored the old sprite.
fn cursor_to_fb(
    desk: &Desktop,
    track: &mut CursorTrack,
    fb: &mut [u8],
    info: FrameBufferInfo,
    n: usize,
) {
    let t0 = trace::t();
    track.paint(desk.pointer(), info.width as i32, info.height as i32);
    let mut c = Canvas::new(&mut fb[..n], info);
    desk.draw_cursor_overlay(&mut c);
    trace::stage(trace::Stage::Cursor, t0);
}

/// Upload the whole back buffer to the framebuffer (timed as `Stage::Blit`).
fn fb_full_blit(fb: &mut [u8], back: &[u8], n: usize) {
    if trace::ON {
        trace::vram_upload(n as u64, trace::count_diff(&fb[..n], &back[..n]));
    }
    let t0 = trace::t();
    fb[..n].copy_from_slice(back);
    trace::stage(trace::Stage::Blit, t0);
}

/// `blit_rect` into the framebuffer (timed as `Stage::Blit`).
#[allow(clippy::too_many_arguments)]
fn fb_blit_rect(
    fb: &mut [u8],
    back: &[u8],
    info: FrameBufferInfo,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    n: usize,
) {
    if trace::ON {
        // Bytes this rect uploads vs how many of them actually differ from what
        // is already in the framebuffer (what a diff-based upload could skip).
        let bpp = info.bytes_per_pixel;
        let (x0, y0) = (x.max(0) as usize, y.max(0) as usize);
        if x0 < info.width && y0 < info.height {
            let x_end = (x0 + w.max(0) as usize).min(info.width);
            let y_end = (y0 + h.max(0) as usize).min(info.height);
            let (mut up, mut chg) = (0u64, 0u64);
            for row in y0..y_end {
                let off = (row * info.stride + x0) * bpp;
                let end = off + (x_end - x0) * bpp;
                if end <= n {
                    up += (end - off) as u64;
                    chg += trace::count_diff(&fb[off..end], &back[off..end]);
                }
            }
            trace::vram_upload(up, chg);
        }
    }
    let t0 = trace::t();
    blit_rect(fb, back, info, x, y, w, h, n);
    trace::stage(trace::Stage::Blit, t0);
}

/// Copy a rectangular region from `src` into `dst` (same framebuffer layout).
/// Used to restore the background under the moving cursor.
#[allow(clippy::too_many_arguments)]
fn blit_rect(
    dst: &mut [u8],
    src: &[u8],
    info: FrameBufferInfo,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    n: usize,
) {
    let bpp = info.bytes_per_pixel;
    let stride = info.stride;
    let x = x.max(0) as usize;
    let y = y.max(0) as usize;
    if x >= info.width || y >= info.height {
        return;
    }
    let x_end = (x + w as usize).min(info.width);
    let y_end = (y + h as usize).min(info.height);
    let row_len = (x_end - x) * bpp;
    for row in y..y_end {
        let off = (row * stride + x) * bpp;
        let end = off + row_len;
        if end <= n {
            dst[off..end].copy_from_slice(&src[off..end]);
        }
    }
}

/// Exercises the heap (alloc, grow, free) once at boot. A broken allocator
/// would fault or hang instead of silently corrupting later.
fn heap_smoke_test() {
    use alloc::vec::Vec;
    let mut v: Vec<u32> = Vec::new();
    for i in 0..1024 {
        v.push(i);
    }
    let sum: u32 = v.iter().sum();
    core::hint::black_box(sum);
    drop(v);

    // Fragmentation check: churn many small allocations, free them, then demand
    // a block larger than any single freed chunk. This only succeeds if the
    // allocator coalesced the freed regions back together.
    let mut chunks: Vec<Vec<u8>> = Vec::new();
    for _ in 0..256 {
        chunks.push(alloc::vec![0u8; 1024]);
    }
    drop(chunks); // frees ~256 KiB in scattered chunks
    let big: Vec<u8> = alloc::vec![7u8; 200 * 1024];
    core::hint::black_box(big.len());
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    // COM1 and the screen both get the report (see `crash`); without this every
    // panic, including allocation failure, was a silent `hlt`.
    // A panic in a plain thread context kills only that thread (see `sched::kill_current`); one in the
    // compositor, in an ISR or with interrupts off, or a second one while killing, is fatal.
    crash::fault(
        crash::Kind::Panic,
        "the kernel panicked",
        format_args!("{info}"),
        None,
        x86_64::instructions::interrupts::are_enabled(),
    )
}
