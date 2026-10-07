//! The `appd` half of the manager: building a runtime, running one slice of a guest,
//! publishing its frame and charging its resources. Everything here runs on the
//! `appd` thread only.

use super::*;

/// Wall clock in ms since local midnight (the RTC has no date or sub-second part).
fn wall_ms() -> u64 {
    let t = x86_64::instructions::interrupts::without_interrupts(crate::rtc::now);
    (t.h as u64 * 3600 + t.m as u64 * 60 + t.s as u64) * 1000
}

pub(super) fn build_runtime(i: usize) -> Result<Box<Runtime>, String> {
    let (wasm, manifest, v1, quotas) = with(|slots| {
        slots[i]
            .as_ref()
            .map(|s| (s.wasm.clone(), s.manifest.clone(), s.v1, s.quotas))
    })
    .ok_or_else(|| String::from("instancia inexistente"))?;
    let engine = guest_engine();
    let module =
        Module::new(&engine, &wasm[..]).map_err(|e| alloc::format!("modulo invalido: {e}"))?;
    let mut state = HostState::with_limits(guest_limits(quotas.mem_bytes));
    let sandbox = Sandbox::new(manifest.fs, &manifest.id, quotas.disk_bytes, quotas.max_fds)
        .ok_or_else(|| String::from("id de app invalido"))?;
    let mono = crate::interrupts::ticks() * 4;
    state.v2 = Some(Box::new(abi2::V2::new(
        &manifest.id,
        sandbox,
        manifest.net,
        manifest.clipboard,
        mono,
        wall_ms(),
    )));
    let mut store = Store::new(&engine, state);
    store.limiter(|st| &mut st.limits);
    let init_fuel = if v1 {
        INIT_FUEL
    } else {
        quotas.fuel_frame.saturating_mul(8).min(64_000_000)
    };
    store
        .set_fuel(init_fuel)
        .map_err(|_| String::from("combustivel"))?;
    let mut linker = <Linker<HostState>>::new(&engine);
    install_all(&mut linker).map_err(String::from)?;
    let instance = linker
        .instantiate_and_start(&mut store, &module)
        .map_err(|e| alloc::format!("instanciacao falhou: {}", describe_load(&e)))?;
    // A WASI "reactor" module (clang -mexec-model=reactor, e.g. DOOM) exposes
    // `_initialize`, which must run once before any other export is called.
    if let Ok(init) = instance.get_typed_func::<(), ()>(&store, "_initialize")
        && let Err(e) = init.call(&mut store, ())
    {
        return Err(alloc::format!("_initialize: {}", describe(&e)));
    }
    let memory = instance
        .get_memory(&store, "memory")
        .or_else(|| instance.get_memory(&store, "mem"));
    Ok(Box::new(Runtime {
        store,
        instance,
        memory,
        first_render: true,
        fuel_spent: 0,
        calls: 0,
        last_buttons: 0,
        redraw_again: false,
    }))
}

fn describe_load(e: &wasmi::Error) -> String {
    // Link/instantiate errors name the missing import; keep the text short.
    let mut s = alloc::format!("{e}");
    s.truncate(120);
    s
}

/// (Re)allocate both surfaces for a `w` x `h` content box. `false` if memory is short.
pub(super) fn resize_surfaces(i: usize, w: i32, h: i32) -> bool {
    let Some(info) = fb_info() else {
        return false;
    };
    let (wu, hu) = (w.max(1) as usize, h.max(1) as usize);
    if wu * hu > MAX_SURFACE_PIXELS {
        return false;
    }
    let n = wu * hu * info.bytes_per_pixel;
    let mut a: Vec<u8> = Vec::new();
    let mut b: Vec<u8> = Vec::new();
    if a.try_reserve_exact(n).is_err() || b.try_reserve_exact(n).is_err() {
        return false;
    }
    a.resize(n, 0);
    b.resize(n, 0);
    let mut spins = 0;
    loop {
        let done = with(|slots| {
            let Some(s) = slots[i].as_mut() else {
                return true;
            };
            if s.reading.is_some() {
                return false;
            }
            s.surf = [core::mem::take(&mut a), core::mem::take(&mut b)];
            s.front = 0;
            s.cur_w = w;
            s.cur_h = h;
            true
        });
        if done {
            return true;
        }
        spins += 1;
        if spins > 10_000 {
            return false;
        }
        crate::sched::yield_now();
    }
}

/// Call guest export `name` with `args`, trimmed to the export's arity (so one name
/// serves both v1 `on_key(code)` and v2 `on_key(code, mods)`). Missing exports are fine.
fn call_export(rt: &mut Runtime, name: &str, args: &[i32], fuel: u64) -> Result<(), wasmi::Error> {
    let Some(f) = rt.instance.get_func(&rt.store, name) else {
        return Ok(());
    };
    let n = f.ty(&rt.store).params().len();
    if n > 4 {
        return Ok(()); // incompatible signature: ignore the hook
    }
    let all = [
        args.first().copied().unwrap_or(0),
        args.get(1).copied().unwrap_or(0),
        args.get(2).copied().unwrap_or(0),
        0,
    ];
    let vals = [
        Val::I32(all[0]),
        Val::I32(all[1]),
        Val::I32(all[2]),
        Val::I32(all[3]),
    ];
    rt.store.set_fuel(fuel)?;
    let r = f.call(&mut rt.store, &vals[..n], &mut []);
    let left = rt.store.get_fuel().unwrap_or(0);
    rt.fuel_spent += fuel.saturating_sub(left);
    rt.calls += 1;
    r
}

/// Run one slice of the guest. `Ok(true)` when it rendered (publish the surface).
pub(super) fn run_guest(i: usize, job: &Job) -> Result<bool, wasmi::Error> {
    let fault = |what: &'static str| wasmi::Error::host(AppFault(what));
    let rt = rts()[i].as_mut().ok_or_else(|| fault("sem runtime"))?;
    // The surface to draw into: the one the compositor is not reading.
    let mut spins = 0;
    let (back, ptr, len, w, h, had_front) = loop {
        let r = with(|slots| {
            let s = slots[i].as_mut()?;
            let back = 1 - s.front;
            if s.reading == Some(back) {
                return Some(None);
            }
            Some(Some((
                back,
                s.surf[back].as_mut_ptr(),
                s.surf[back].len(),
                s.cur_w,
                s.cur_h,
                s.ready,
            )))
        });
        match r {
            None => return Err(fault("instancia removida")),
            Some(Some(x)) => break x,
            Some(None) => {
                spins += 1;
                if spins > 10_000 {
                    return Err(fault("superficie ocupada"));
                }
                crate::sched::yield_now();
            }
        }
    };
    let info = surface_info(w, h).ok_or_else(|| fault("sem framebuffer"))?;
    // Packaged (v2) apps may draw incrementally: start from the last published frame.
    if !job.v1 && had_front {
        with(|slots| {
            if let Some(s) = slots[i].as_mut() {
                let (a, b) = s.surf.split_at_mut(1);
                let (front, backv) = if back == 1 {
                    (&a[0], &mut b[0])
                } else {
                    (&b[0], &mut a[0])
                };
                if front.len() == backv.len() {
                    backv.copy_from_slice(front);
                }
            }
        });
    }
    rt.store.data_mut().set_surface(ptr, len, info);
    let fuel = job.quotas.fuel_frame.min(FRAME_FUEL);
    let result = guest_slice(rt, job, fuel, w, h);
    rt.store.data_mut().clear_surface();
    result
}

fn guest_slice(
    rt: &mut Runtime,
    job: &Job,
    fuel: u64,
    w: i32,
    h: i32,
) -> Result<bool, wasmi::Error> {
    let mut want_render = if job.v1 {
        job.frame || job.starting
    } else {
        job.redraw || job.starting
    };
    if !job.v1 && job.resize.is_some() {
        call_export(rt, "on_resize", &[w, h], fuel)?;
        want_render = true;
    }
    for ev in &job.events {
        want_render |= dispatch(rt, *ev, job.v1, fuel)?;
    }
    if let Some(dt) = job.tick {
        call_export(rt, "on_tick", &[dt], fuel)?;
        want_render = true;
    }
    if let Some(v) = rt.store.data_mut().v2.as_deref_mut()
        && core::mem::take(&mut v.redraw)
    {
        want_render = true;
    }
    if !want_render {
        return Ok(false);
    }
    let first = core::mem::take(&mut rt.first_render);
    let render_fuel = if job.v1 && first {
        INIT_FUEL
    } else if first {
        fuel.saturating_mul(8).min(64_000_000)
    } else {
        fuel
    };
    call_export(rt, "render", &[], render_fuel)?;
    // A guest that asked for a redraw during `render` gets one more pass.
    if let Some(v) = rt.store.data_mut().v2.as_deref_mut()
        && core::mem::take(&mut v.redraw)
    {
        rt.redraw_again = true;
    }
    Ok(true)
}

/// Deliver one input event. Returns whether the app should redraw afterwards.
fn dispatch(rt: &mut Runtime, ev: Ev, v1: bool, fuel: u64) -> Result<bool, wasmi::Error> {
    match ev {
        Ev::Key(code, mods) => {
            if v1 && !(0x20..0x7F).contains(&code) && code != 10 && code != 27 {
                return Ok(false);
            }
            call_export(rt, "on_key", &[code, mods], fuel)?;
            Ok(true)
        }
        Ev::Text(cp) => {
            if v1 {
                return Ok(false);
            }
            call_export(rt, "on_text", &[cp], fuel)?;
            Ok(true)
        }
        Ev::Pointer(x, y, b) => {
            if v1 {
                // v1 apps only know "clicked": deliver presses, as before.
                let press = b & 1 != 0 && rt.last_buttons & 1 == 0;
                rt.last_buttons = b;
                if !press {
                    return Ok(false);
                }
                call_export(rt, "on_pointer", &[x, y, 1], fuel)?;
                return Ok(true);
            }
            rt.last_buttons = b;
            call_export(rt, "on_pointer", &[x, y, b], fuel)?;
            Ok(true)
        }
        Ev::Close => {
            if !v1 {
                call_export(rt, "on_close", &[], fuel)?;
            }
            Ok(false)
        }
    }
}

/// Publish the back surface as the new front frame.
pub(super) fn publish(i: usize) {
    let now = crate::interrupts::ticks();
    let again = rts()[i]
        .as_mut()
        .is_some_and(|rt| core::mem::take(&mut rt.redraw_again));
    with(|slots| {
        if let Some(s) = slots[i].as_mut() {
            s.front = 1 - s.front;
            s.ready = true;
            s.frames += 1;
            s.last_frame = now;
            if again {
                s.want_redraw = true;
            }
        }
    });
}

/// Charge the slice's CPU, fuel and memory to the app.
pub(super) fn account(i: usize, cycles: u64, now: u64) {
    let (fuel, calls, mem_kib) = match rts()[i].as_mut() {
        Some(rt) => {
            let pages = rt.memory.map_or(0, |m| m.size(&rt.store));
            (
                core::mem::take(&mut rt.fuel_spent),
                core::mem::take(&mut rt.calls),
                (pages * 64).min(u32::MAX as u64) as u32,
            )
        }
        None => (0, 0, 0),
    };
    with(|slots| {
        if let Some(s) = slots[i].as_mut() {
            s.cpu_total = s.cpu_total.wrapping_add(cycles);
            s.cpu_win = s.cpu_win.saturating_add(cycles);
            s.fuel_total = s.fuel_total.saturating_add(fuel);
            s.calls += calls;
            if mem_kib != 0 {
                s.mem_kib = mem_kib;
                s.mem_peak_kib = s.mem_peak_kib.max(mem_kib);
            }
            roll_cpu(s, now);
        }
    });
}

/// Copy a guest-set window title into the slot.
pub(super) fn sync_title(i: usize) {
    let Some(rt) = rts()[i].as_mut() else { return };
    let Some(v) = rt.store.data_mut().v2.as_deref_mut() else {
        return;
    };
    if core::mem::take(&mut v.title_dirty) {
        let t = v.title.clone();
        with(|slots| {
            if let Some(s) = slots[i].as_mut() {
                s.title = t;
            }
        });
    }
}
