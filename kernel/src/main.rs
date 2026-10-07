#![no_std]
#![no_main]
#![warn(clippy::undocumented_unsafe_blocks)]
#![feature(abi_x86_interrupt)]

extern crate alloc;

mod allocator;
mod ata;
mod boot;
mod desktop;
mod fb;
mod fetch;
mod font;
mod gdt;
mod icons;
mod interrupts;
mod io;
mod logo;
mod ne2000;
mod netstack;
mod pci;
mod perf;
mod power;
mod ps2;
mod rtc;
mod sched;
mod serial;
mod sync;
mod theme;
mod trace;
mod virtio;
mod virtio_gpu;
mod wasm;

use bootloader_api::config::{BootloaderConfig, Mapping};
use bootloader_api::info::FrameBufferInfo;
use bootloader_api::{BootInfo, entry_point};
use core::panic::PanicInfo;
use desktop::{CURSOR_H, CURSOR_W, Desktop};
use fb::Canvas;
use osjeff_core::net::{self, Ipv4};
use osjeff_core::{Rect, Time};

/// Our IPv4 address. Matches QEMU's user-mode (SLIRP) default guest address.
const NET_IP: Ipv4 = Ipv4([10, 0, 2, 15]);
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
    serial_println!("OSjeff boot: kernel entry");

    // Capture the physical-memory offset before `boot_info` is borrowed for the
    // framebuffer. Needed for DMA (virtio-gpu addresses memory physically).
    let phys_offset = boot_info.physical_memory_offset.into_option();
    serial_println!("physical_memory_offset: {:#x?}", phys_offset);

    let framebuffer = match boot_info.framebuffer.as_mut() {
        Some(fb) => fb,
        None => {
            serial_println!("FATAL: bootloader provided no framebuffer");
            halt()
        }
    };
    let info = framebuffer.info();
    let n = framebuffer.buffer().len().min(MAX_BYTES);

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
    serial_println!("OSjeff boot: running native WASM demo");
    wasm::run_demo();
    trace::mark("wasm demo done");

    // Enumerate the PCI bus — groundwork for the virtio-gpu driver: locate the
    // device and, when present, enable bus mastering so a later DMA-capable
    // driver can use it. QEMU captures the log via `-serial file:...`.
    serial_println!("OSjeff boot: enumerating PCI bus 0");
    pci::for_each(|d| {
        serial_println!(
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
            serial_println!(
                "virtio-gpu @ slot {} func {} bar0={:#010x}",
                gpu.slot,
                gpu.func,
                gpu.bar(0)
            );
            match virtio::discover(&gpu) {
                Some(caps) => {
                    serial_println!(
                        "  common bar{} off={:#x} len={}",
                        caps.common.bar,
                        caps.common.offset,
                        caps.common.length
                    );
                    serial_println!(
                        "  notify bar{} off={:#x} mul={}",
                        caps.notify.bar,
                        caps.notify.offset,
                        caps.notify_off_mul
                    );
                    serial_println!(
                        "  isr    bar{} off={:#x}   device bar{} off={:#x}",
                        caps.isr.bar,
                        caps.isr.offset,
                        caps.device.bar,
                        caps.device.offset
                    );
                }
                None => serial_println!("  virtio caps: none (transitional device?)"),
            }

            // Bring up the full virtio-gpu driver: negotiate, set up the control
            // queue (DMA), flip DRIVER_OK, and query the display geometry — which
            // exercises the entire command path end to end.
            if let (Some(off), Some(caps)) = (phys_offset, virtio::discover(&gpu)) {
                match virtio_gpu::GpuDevice::init(&gpu, caps, off) {
                    Some(mut dev) => {
                        serial_println!("virtio-gpu: control queue up, DRIVER_OK");
                        match dev.get_display_info() {
                            Some((w, h)) => {
                                serial_println!("virtio-gpu display 0: {}x{}", w, h)
                            }
                            None => serial_println!("virtio-gpu: get_display_info failed"),
                        }
                        // Exercise the 2D command path (no scanout swap — the VBE
                        // display stays live). The accelerated scanout is the next step.
                        dev.verify_2d();
                    }
                    None => serial_println!("virtio-gpu: init failed"),
                }
            }
        }
        None => serial_println!("virtio-gpu: absent — using the VBE framebuffer"),
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
    serial_println!("TSC calibrated: {} kHz", tsc_khz);
    trace::mark("tsc calibrated (25 PIT ticks)");
    trace::calibrated(tsc_khz);
    trace::bench_prims(back, info, tsc_khz);
    trace::bench_alloc(tsc_khz);
    let mut perf = perf::Perf::new(tsc_khz);

    // Bring up the NIC (if present), lease an IP over DHCP (falling back to the
    // static address if no server answers), and announce ourselves with a
    // gratuitous ARP so the network is visible on the wire from boot. No card ->
    // skip and keep the static IP.
    let net_up = ne2000::init();
    trace::mark("ne2000 init done");
    let net_ip = if net_up { dhcp_acquire(NET_IP) } else { NET_IP };
    trace::mark("dhcp done");
    if net_up {
        let mut frame = [0u8; 64];
        let len = net::arp_announce(&mut frame, ne2000::MAC, net_ip);
        ne2000::send(&frame[..len]);
    }

    // Hand the browser's TCP/IP + TLS stack to the background fetcher and spawn
    // its worker thread, so page loads (and the slow software TLS handshake) run
    // off the compositor thread and never freeze the UI. Interrupts are disabled
    // around `spawn` because it mutates the scheduler's thread list, which the
    // timer ISR also reads.
    if net_up {
        fetch::init(netstack::Net::new());
        x86_64::instructions::interrupts::without_interrupts(|| {
            sched::spawn("fetcher", fetch::worker);
        });
    }

    // Hand the framebuffer layout to the WASM app engine and spawn its worker
    // thread, so a heavy app (DOOM loading its WAD, then running its game loop)
    // renders off the compositor thread and never freezes the UI.
    wasm::init(info);
    x86_64::instructions::interrupts::without_interrupts(|| {
        sched::spawn("wasmapp", wasm::worker);
    });

    trace::mark("threads spawned (fetcher, wasmapp)");

    // Boot splash: progress tracks real elapsed time (>= 5 seconds).
    run_splash(&mut *framebuffer, &mut *back, info, n);
    trace::mark("splash end (artificial >= 5 s)");

    // Static layer painted once.
    {
        let mut c = Canvas::new(bg, info);
        desktop::paint_background(&mut c);
    }
    trace::mark("wallpaper painted");

    let mut desk = Desktop::new(info.width as i32, info.height as i32);
    trace::mark("Desktop::new (fs load from ATA) done");
    let mut last_sec = 0xFFu8; // force first render
    let mut prev_cursor = desk.cursor();
    // Seed from the live tick count, NOT 0: the splash ran for ~5 s with the
    // timer firing, so a 0 seed would make the first frame's delta enormous and
    // instantly complete the open animation (skipping the full-screen blit that
    // clears the splash).
    let mut last_tick = interrupts::ticks();

    // Animation fast-path state. `was_anim` starts true so the first steady
    // frame forces one full repaint over the splash even if no animation runs.
    let mut was_anim = true;
    let mut static_valid = false;
    let mut last_sig = 0u64;
    let mut prev_damage = Rect::new(0, 0, 0, 0);
    // Focused window rect from the previous steady frame, so a content change can
    // also repaint the window that just lost focus (its title de-highlights).
    let mut prev_focused: Option<Rect> = None;
    let mut last_hud = 0u64; // tick of the last perf-HUD refresh
    // Set after a browser fetch completes so the next iteration repaints the
    // page (the fetch itself blocks, so it can't render in its own frame).
    let mut browser_redraw = false;

    // Wall-clock animation speed, independent of how often the GUI thread is
    // scheduled (the timer preempts round-robin across all threads).
    const DT_PER_TICK: f32 = 0.03;
    let mut first_frame = true;

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
        }

        // Run the WASM app worker only while its window is open.
        wasm::set_active(desk.wasm_active());

        let any_anim = desk.has_animation();

        // The cheap clock-tick repaint (rect-only blit) is only valid in the
        // steady desktop. While an overlay or animation is up, fold the tick
        // into a normal recompose so those transient layers stay consistent.
        if clock_tick && (any_anim || desk.overlay_open()) {
            scene_dirty = true;
            clock_tick = false;
        }

        // Did this iteration do real rendering work? (Used to time frames.)
        let work = any_anim || scene_dirty || clock_tick || cursor_moved || was_anim;
        let frame_start = io::rdtsc();
        let cpu_start = trace::cpu_now();
        let mut path: Option<trace::Path> = None;

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
                cursor_to_fb(&desk, framebuffer.buffer_mut(), info, n);
                prev_cursor = desk.cursor();
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
                // Repaint the cursor (the damage blit may have covered it, and the
                // cursor itself may have moved).
                let (ox, oy) = prev_cursor;
                fb_blit_rect(
                    framebuffer.buffer_mut(),
                    back,
                    info,
                    ox,
                    oy,
                    CURSOR_W,
                    CURSOR_H,
                    n,
                );
                cursor_to_fb(&desk, framebuffer.buffer_mut(), info, n);
                prev_cursor = desk.cursor();
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
                cursor_to_fb(&desk, framebuffer.buffer_mut(), info, n);
                prev_cursor = desk.cursor();
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
                    desk.draw_overlay(&mut c);
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
                // Repaint the cursor (it may have moved off the overlay).
                let (ox, oy) = prev_cursor;
                fb_blit_rect(
                    framebuffer.buffer_mut(),
                    back,
                    info,
                    ox,
                    oy,
                    CURSOR_W,
                    CURSOR_H,
                    n,
                );
                cursor_to_fb(&desk, framebuffer.buffer_mut(), info, n);
                prev_cursor = desk.cursor();
            }
        } else {
            static_valid = false;
            // A finished animation needs one final full recompose to settle.
            if was_anim {
                scene_dirty = true;
            }

            if scene_dirty {
                path = Some(if was_anim {
                    trace::Path::Settle
                } else {
                    trace::Path::Steady
                });
                let tc = trace::t();
                copy_bg(back, bg);
                desk.render(back, info, time);
                trace::stage(trace::Stage::Compose, tc);
                if was_anim {
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
                    up(desk.clock_rect());
                }
                cursor_to_fb(&desk, framebuffer.buffer_mut(), info, n);
                prev_cursor = desk.cursor();
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
                // The cursor isn't in `back`; repaint it in case it overlaps the
                // regions just blitted.
                let (ox, oy) = prev_cursor;
                fb_blit_rect(
                    framebuffer.buffer_mut(),
                    back,
                    info,
                    ox,
                    oy,
                    CURSOR_W,
                    CURSOR_H,
                    n,
                );
                cursor_to_fb(&desk, framebuffer.buffer_mut(), info, n);
                prev_cursor = desk.cursor();
            } else if cursor_moved {
                path = Some(trace::Path::Cursor);
                // Cheap path: restore under the old cursor, draw at the new spot.
                let (ox, oy) = prev_cursor;
                fb_blit_rect(
                    framebuffer.buffer_mut(),
                    back,
                    info,
                    ox,
                    oy,
                    CURSOR_W,
                    CURSOR_H,
                    n,
                );
                cursor_to_fb(&desk, framebuffer.buffer_mut(), info, n);
                prev_cursor = desk.cursor();
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
            }
        }

        // Perf HUD: refresh ~10x/s as a framebuffer overlay restored from `back`,
        // so it never pollutes the cached scene. Drawn outside the timed window.
        if tick.saturating_sub(last_hud) >= 25 {
            last_hud = tick;
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
        if report_due {
            trace::report(tsc_khz, tick);
        }

        // Browser navigation: hand any pending request to the background fetcher
        // (non-blocking — the worker thread does the slow fetch while we keep
        // rendering the "Carregando" state), and pick up a finished result.
        if fetch::is_idle() {
            let mut url = [0u8; 256];
            if let Some(len) = desk.browser_take_request(&mut url) {
                fetch::try_post(&url[..len]);
            }
        }
        if let Some(result) = fetch::take_result() {
            match result {
                Some(r) => desk.browser_load(&r),
                None => desk.browser_fail(),
            }
            browser_redraw = true;
        }

        // Service the network: answer ARP/ping for received frames — but only
        // while no fetch is running, so the NIC has a single owner (the worker
        // owns it during a fetch).
        if net_up && fetch::is_idle() {
            let mut rx = [0u8; 1600];
            while let Some(len) = ne2000::poll(&mut rx) {
                let mut tx = [0u8; 1600];
                if let Some(reply) = net::respond(&rx[..len], ne2000::MAC, net_ip, &mut tx) {
                    ne2000::send(&tx[..reply]);
                }
            }
        }

        // Idle until the next interrupt instead of busy-spinning. The timer
        // (250 Hz) wakes us to step animations and the per-second clock; the
        // keyboard/mouse IRQs wake us immediately on input. This paces frames to
        // the tick rate and stops the compositor from burning a full core — a
        // real system halts when it has nothing to draw.
        x86_64::instructions::hlt();
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

/// Draw the cursor straight into the framebuffer (timed as `Stage::Cursor`).
fn cursor_to_fb(desk: &Desktop, fb: &mut [u8], info: FrameBufferInfo, n: usize) {
    let t0 = trace::t();
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

/// Acquire an IP via DHCP (DISCOVER -> OFFER -> REQUEST -> ACK). Every wait is
/// time-bounded against the timer tick, so a missing or slow server never hangs
/// the boot — it just falls back to `default` (the static address).
fn dhcp_acquire(default: Ipv4) -> Ipv4 {
    let xid = (io::rdtsc() as u32) | 1; // any non-zero transaction id
    let mut tx = [0u8; 600];
    let mut rx = [0u8; 1600];

    let len = net::dhcp_discover(&mut tx, ne2000::MAC, xid);
    ne2000::send(&tx[..len]);
    let offer = match poll_dhcp(&mut rx, xid, net::DHCP_OFFER) {
        Some(o) => o,
        None => return default,
    };

    let len = net::dhcp_request(&mut tx, ne2000::MAC, xid, offer.your_ip, offer.server_id);
    ne2000::send(&tx[..len]);
    match poll_dhcp(&mut rx, xid, net::DHCP_ACK) {
        Some(ack) => ack.your_ip,
        None => default,
    }
}

/// Poll the NIC for up to ~300 ms for a DHCP reply of type `want` matching `xid`.
fn poll_dhcp(rx: &mut [u8], xid: u32, want: u8) -> Option<net::DhcpReply> {
    let deadline = interrupts::ticks() + 75; // 75 ticks / 250 Hz ≈ 300 ms
    while interrupts::ticks() < deadline {
        if let Some(n) = ne2000::poll(rx)
            && let Some(r) = net::parse_dhcp(&rx[..n], ne2000::MAC)
            && r.xid == xid
            && r.msg_type == want
        {
            return Some(r);
        }
    }
    None
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

fn halt() -> ! {
    loop {
        // SAFETY: `hlt` is legal in ring 0 and touches no memory; it only parks the CPU.
        unsafe { core::arch::asm!("hlt", options(nomem, nostack, preserves_flags)) };
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    // Serial is the one channel that survives a dead compositor; without this
    // every panic (including allocation failure) was a silent `hlt`.
    serial_println!("KERNEL PANIC: {info}");
    halt();
}
