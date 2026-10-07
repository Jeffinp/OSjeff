//! ABI v2: the `osj.*` host functions.
//!
//! Every function validates what the guest hands it before touching anything
//! (`osjeff_core::appabi::check_range`): a pointer/length outside the guest's
//! linear memory raises an [`AppFault`] trap and **only that app dies** ("ponteiro
//! invalido"); a length above the function's cap returns `ERR_INVAL`; a denied
//! permission returns `ERR_PERM`. Host work the fuel meter cannot see (pixel
//! fills, image decoding, file I/O) is charged to the guest's fuel.
//!
//! The decisions (paths, quotas, descriptors, URL filter) live in
//! `osjeff_core::{appfs, appnet}`; this file only moves bytes between the guest's
//! memory and those pure pieces.

use super::{AppFault, HostState, appfs_backend, charge, guest_mem, host_fill, host_text, manager};
use crate::interrupts;
use alloc::string::String;
use alloc::vec::Vec;
use core::ops::Range;
use osjeff_core::appabi::*;
use osjeff_core::appfs::Sandbox;
use osjeff_core::appmanifest::{ClipPerm, NetPerm};
use osjeff_core::appnet;
use wasmi::{Caller, Linker, Memory};

type C<'a> = Caller<'a, HostState>;
type R<T> = Result<T, wasmi::Error>;

/// Per-app v2 state, in the `Store`'s `HostState`.
pub(crate) struct V2 {
    pub id: String,
    pub sandbox: Sandbox,
    pub net: NetPerm,
    pub clip: ClipPerm,
    pub title: String,
    pub title_dirty: bool,
    /// The guest asked for a redraw (`request_redraw`).
    pub redraw: bool,
    pub rng: u32,
    pub log_left: usize,
    pub limiter: appnet::Limiter,
    /// Monotonic ms at start, and the wall clock (ms since local midnight) then.
    pub mono0_ms: u64,
    pub wall0_ms: u64,
    /// Path-escape attempts already reported to the serial log.
    pub escapes_logged: u32,
}

impl V2 {
    pub(crate) fn new(
        id: &str,
        sandbox: Sandbox,
        net: NetPerm,
        clip: ClipPerm,
        mono_ms: u64,
        wall_ms: u64,
    ) -> V2 {
        // xorshift32 must not start at 0; mix the id and the clock.
        let mut seed = (mono_ms as u32) ^ 0x9E37_79B9;
        for b in id.bytes() {
            seed = seed.rotate_left(5) ^ (b as u32).wrapping_mul(0x85EB_CA6B);
        }
        V2 {
            id: String::from(id),
            sandbox,
            net,
            clip,
            title: String::new(),
            title_dirty: false,
            redraw: false,
            rng: if seed == 0 { 0x1234_5678 } else { seed },
            log_left: LOG_BUDGET,
            limiter: appnet::Limiter::new(),
            mono0_ms: mono_ms,
            wall0_ms: wall_ms,
            escapes_logged: 0,
        }
    }
}

fn fault(what: &'static str) -> wasmi::Error {
    wasmi::Error::host(AppFault(what))
}

fn mono_ms() -> u64 {
    interrupts::ticks() * 4
}

fn v2<'a>(c: &'a mut Caller<'_, HostState>) -> R<&'a mut V2> {
    c.data_mut()
        .v2
        .as_deref_mut()
        .ok_or_else(|| fault("sem ABI v2"))
}

/// Memory + validated range. A bad range is a fault (the app dies).
fn range(c: &C, ptr: i32, len: i32) -> R<(Memory, Range<usize>)> {
    let mem = guest_mem(c).ok_or_else(|| fault("sem memoria exportada"))?;
    let r = check_range(mem.data_size(c), ptr, len).map_err(|_| fault("ponteiro invalido"))?;
    Ok((mem, r))
}

/// Copy `[ptr, ptr+len)` out of guest memory. `Ok(Err(code))` when `len > cap`.
fn read_capped(c: &C, ptr: i32, len: i32, cap: usize) -> R<Result<Vec<u8>, i32>> {
    let mem = guest_mem(c).ok_or_else(|| fault("sem memoria exportada"))?;
    let r =
        check_capped(mem.data_size(c), ptr, len, cap).map_err(|_| fault("ponteiro invalido"))?;
    Ok(r.map(|r| mem.data(c)[r].to_vec()))
}

fn write_guest(c: &mut C, ptr: i32, bytes: &[u8]) -> R<()> {
    let (mem, r) = range(c, ptr, bytes.len() as i32)?;
    mem.data_mut(&mut *c)[r].copy_from_slice(bytes);
    Ok(())
}

macro_rules! reg {
    ($linker:expr, $name:literal, $f:expr) => {
        $linker
            .func_wrap("osj", $name, $f)
            .map_err(|_| concat!("link osj.", $name))?;
    };
}

/// Register every `osj.*` function.
pub(crate) fn install(l: &mut Linker<HostState>) -> Result<(), &'static str> {
    // ---------------------------------------------------------------- window
    reg!(l, "set_title", |mut c: C, ptr: i32, len: i32| -> R<i32> {
        let bytes = match read_capped(&c, ptr, len, MAX_TITLE)? {
            Ok(b) => b,
            Err(e) => return Ok(e),
        };
        if !bytes.iter().all(|&b| (0x20..=0x7E).contains(&b)) {
            return Ok(ERR_INVAL);
        }
        let st = v2(&mut c)?;
        st.title = bytes.iter().map(|&b| b as char).collect();
        st.title_dirty = true;
        Ok(0)
    });
    reg!(l, "get_size", |c: C| -> i64 {
        let st = c.data();
        ((st.cw as i64) << 32) | (st.ch as u32 as i64)
    });
    reg!(l, "request_redraw", |mut c: C| -> R<()> {
        v2(&mut c)?.redraw = true;
        Ok(())
    });

    // ---------------------------------------------------------------- drawing
    reg!(l, "fill_rect", |mut c: C,
                          x: i32,
                          y: i32,
                          w: i32,
                          h: i32,
                          color: i32|
     -> R<()> {
        if w > 0 && h > 0 {
            charge(&mut c, (w as u64 * h as u64 / 256 + 1).min(1 << 20))?;
        }
        host_fill(c.data(), x, y, w, h, color);
        Ok(())
    });
    reg!(l, "draw_text", |mut c: C,
                          x: i32,
                          y: i32,
                          ptr: i32,
                          len: i32,
                          color: i32,
                          scale: i32|
     -> R<i32> {
        let bytes = match read_capped(&c, ptr, len, MAX_TEXT)? {
            Ok(b) => b,
            Err(e) => return Ok(e),
        };
        charge(&mut c, bytes.len() as u64 * 8)?;
        // Non-ASCII bytes draw as the font's replacement glyph; never an error.
        let s: String = bytes.iter().map(|&b| b as char).collect();
        host_text(c.data(), &s, x, y, color, scale);
        Ok(0)
    });
    reg!(l, "blit_rgba", |mut c: C,
                          ptr: i32,
                          w: i32,
                          h: i32,
                          dx: i32,
                          dy: i32|
     -> R<i32> {
        if w <= 0 || h <= 0 || (w as u64) * (h as u64) > MAX_BLIT_PIXELS {
            return Ok(ERR_INVAL);
        }
        let (mem, r) = range(&c, ptr, w * h * 4)?;
        charge(&mut c, (w as u64) * (h as u64) / 8)?;
        let (data, st) = mem.data_and_store_mut(&mut c);
        blit_1to1(st, &data[r], w, h, dx, dy);
        Ok(0)
    });
    reg!(l, "draw_image_png", |mut c: C,
                               ptr: i32,
                               len: i32,
                               x: i32,
                               y: i32|
     -> R<i32> {
        let bytes = match read_capped(&c, ptr, len, MAX_PNG_BYTES)? {
            Ok(b) => b,
            Err(e) => return Ok(e),
        };
        let Ok(h) = osjeff_core::png::read_header(&bytes) else {
            return Ok(ERR_INVAL);
        };
        if h.width > MAX_PNG_DIM || h.height > MAX_PNG_DIM {
            return Ok(ERR_INVAL);
        }
        charge(&mut c, (h.width as u64) * (h.height as u64) / 2 + 1000)?;
        let Ok(img) = osjeff_core::png::decode(&bytes) else {
            return Ok(ERR_INVAL);
        };
        let Ok(rgba) = img.to_rgba() else {
            return Ok(ERR_INVAL);
        };
        blit_alpha(
            c.data(),
            &rgba,
            img.width() as i32,
            img.height() as i32,
            x,
            y,
        );
        Ok(((img.width() as i32) << 16) | img.height() as i32)
    });

    // ---------------------------------------------------------------- time, random, log, exit
    reg!(l, "now_ms", |mut c: C| -> R<i64> {
        let st = v2(&mut c)?;
        Ok((st.wall0_ms + mono_ms().saturating_sub(st.mono0_ms)) as i64)
    });
    reg!(l, "monotonic_ms", |_: C| -> i64 { mono_ms() as i64 });
    reg!(l, "random", |mut c: C| -> R<i32> {
        let st = v2(&mut c)?;
        let mut x = st.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        st.rng = x;
        Ok(x as i32)
    });
    reg!(l, "log", |mut c: C, ptr: i32, len: i32| -> R<i32> {
        let bytes = match read_capped(&c, ptr, len, MAX_LOG)? {
            Ok(b) => b,
            Err(e) => return Ok(e),
        };
        let st = v2(&mut c)?;
        if bytes.len() > st.log_left {
            st.log_left = 0;
            return Ok(ERR_NOSPC);
        }
        st.log_left -= bytes.len();
        // Printable ASCII only: a guest cannot inject escape sequences into the log.
        let s: String = bytes
            .iter()
            .map(|&b| {
                if (0x20..=0x7E).contains(&b) {
                    b as char
                } else {
                    '.'
                }
            })
            .collect();
        crate::serial_println!("[app {}] {}", st.id, s);
        Ok(0)
    });
    reg!(l, "exit", |_: C, code: i32| -> R<()> {
        Err(wasmi::Error::i32_exit(code))
    });

    // ---------------------------------------------------------------- clipboard
    reg!(l, "clip_get", |mut c: C, ptr: i32, cap: i32| -> R<i32> {
        if v2(&mut c)?.clip != ClipPerm::Rw {
            return Ok(ERR_PERM);
        }
        let cap = cap.clamp(0, 256);
        let (_, r) = range(&c, ptr, cap)?;
        let mut tmp = [0u8; 256];
        let n = manager::clip_get(&mut tmp[..r.len()]);
        write_guest(&mut c, ptr, &tmp[..n])?;
        Ok(n as i32)
    });
    reg!(l, "clip_set", |mut c: C, ptr: i32, len: i32| -> R<i32> {
        if v2(&mut c)?.clip != ClipPerm::Rw {
            return Ok(ERR_PERM);
        }
        let bytes = match read_capped(&c, ptr, len, 256)? {
            Ok(b) => b,
            Err(e) => return Ok(e),
        };
        manager::clip_set(&bytes);
        Ok(0)
    });

    // ---------------------------------------------------------------- files
    reg!(l, "fs_open", |mut c: C,
                        p: i32,
                        n: i32,
                        flags: i32|
     -> R<i32> {
        let path = match read_capped(&c, p, n, MAX_PATH_ARG)? {
            Ok(b) => b,
            Err(e) => return Ok(e),
        };
        let st = v2(&mut c)?;
        let r = appfs_backend::try_with(|fs| st.sandbox.open(fs, &path, flags as u32));
        let code = res(r);
        report_escapes(st);
        charge(&mut c, 200)?;
        Ok(code)
    });
    reg!(l, "fs_close", |mut c: C, fd: i32| -> R<i32> {
        let st = v2(&mut c)?;
        Ok(res(st.sandbox.close(fd).map(|()| 0)))
    });
    reg!(l, "fs_read", |mut c: C,
                        fd: i32,
                        ptr: i32,
                        len: i32|
     -> R<i32> {
        if len < 0 {
            return Err(fault("ponteiro invalido"));
        }
        let want = (len as usize).min(MAX_IO);
        let (mem, r) = range(&c, ptr, want as i32)?;
        let (data, st) = mem.data_and_store_mut(&mut c);
        let Some(v) = st.v2.as_deref_mut() else {
            return Err(fault("sem ABI v2"));
        };
        let out = appfs_backend::try_with(|fs| v.sandbox.read(fs, fd, &mut data[r]));
        let code = res(out.map(|n| n as i32));
        charge(&mut c, 100 + want as u64 / 64)?;
        Ok(code)
    });
    reg!(l, "fs_write", |mut c: C,
                         fd: i32,
                         ptr: i32,
                         len: i32|
     -> R<i32> {
        if len < 0 {
            return Err(fault("ponteiro invalido"));
        }
        let want = (len as usize).min(MAX_IO);
        let (mem, r) = range(&c, ptr, want as i32)?;
        let (data, st) = mem.data_and_store_mut(&mut c);
        let Some(v) = st.v2.as_deref_mut() else {
            return Err(fault("sem ABI v2"));
        };
        let out = appfs_backend::try_with(|fs| v.sandbox.write(fs, fd, &data[r]));
        let code = res(out.map(|n| n as i32));
        charge(&mut c, 100 + want as u64 / 64)?;
        Ok(code)
    });
    reg!(l, "fs_seek", |mut c: C,
                        fd: i32,
                        off: i64,
                        whence: i32|
     -> R<i64> {
        let st = v2(&mut c)?;
        let r = appfs_backend::try_with(|fs| st.sandbox.seek(fs, fd, off, whence));
        Ok(match r {
            Ok(p) => p as i64,
            Err(e) => e.code() as i64,
        })
    });
    reg!(l, "fs_stat", |mut c: C,
                        p: i32,
                        n: i32,
                        out: i32|
     -> R<i32> {
        let path = match read_capped(&c, p, n, MAX_PATH_ARG)? {
            Ok(b) => b,
            Err(e) => return Ok(e),
        };
        range(&c, out, 16)?; // validate before doing the work
        let st = v2(&mut c)?;
        let r = appfs_backend::try_with(|fs| st.sandbox.stat(fs, &path));
        report_escapes(st);
        match r {
            Ok(s) => {
                let mut b = [0u8; 16];
                b[..4].copy_from_slice(&(s.kind as u32).to_le_bytes());
                b[4..12].copy_from_slice(&s.size.to_le_bytes());
                write_guest(&mut c, out, &b)?;
                Ok(0)
            }
            Err(e) => Ok(e.code()),
        }
    });
    reg!(l, "fs_readdir", |mut c: C,
                           p: i32,
                           n: i32,
                           index: i32,
                           out: i32,
                           cap: i32|
     -> R<i32> {
        let path = match read_capped(&c, p, n, MAX_PATH_ARG)? {
            Ok(b) => b,
            Err(e) => return Ok(e),
        };
        if !(2..=256).contains(&cap) {
            return Ok(ERR_INVAL);
        }
        range(&c, out, cap)?;
        if index < 0 {
            return Ok(ERR_INVAL);
        }
        let st = v2(&mut c)?;
        let r = appfs_backend::try_with(|fs| st.sandbox.read_dir(fs, &path, index as usize));
        report_escapes(st);
        match r {
            Ok(Some(e)) => {
                let name = e.name.as_bytes();
                if name.len() + 1 > cap as usize {
                    return Ok(ERR_INVAL);
                }
                let mut b = Vec::with_capacity(name.len() + 1);
                b.push(e.kind as u8);
                b.extend_from_slice(name);
                write_guest(&mut c, out, &b)?;
                Ok(name.len() as i32)
            }
            Ok(None) => Ok(0),
            Err(e) => Ok(e.code()),
        }
    });
    reg!(l, "fs_mkdir", |mut c: C, p: i32, n: i32| -> R<i32> {
        path_op(&mut c, p, n, |sb, fs, path| sb.mkdir(fs, path))
    });
    reg!(l, "fs_unlink", |mut c: C, p: i32, n: i32| -> R<i32> {
        path_op(&mut c, p, n, |sb, fs, path| sb.unlink(fs, path))
    });
    reg!(l, "fs_rename", |mut c: C,
                          a: i32,
                          an: i32,
                          b: i32,
                          bn: i32|
     -> R<i32> {
        let from = match read_capped(&c, a, an, MAX_PATH_ARG)? {
            Ok(b) => b,
            Err(e) => return Ok(e),
        };
        let to = match read_capped(&c, b, bn, MAX_PATH_ARG)? {
            Ok(b) => b,
            Err(e) => return Ok(e),
        };
        let st = v2(&mut c)?;
        let r = appfs_backend::try_with(|fs| st.sandbox.rename(fs, &from, &to));
        report_escapes(st);
        Ok(res(r.map(|()| 0)))
    });

    // ---------------------------------------------------------------- network
    reg!(l, "net_http_get", |mut c: C,
                             url: i32,
                             ulen: i32,
                             out: i32,
                             cap: i32|
     -> R<i32> {
        let bytes = match read_capped(&c, url, ulen, MAX_URL)? {
            Ok(b) => b,
            Err(e) => return Ok(e),
        };
        range(&c, out, cap.clamp(0, appnet::MAX_BODY as i32))?;
        let st = v2(&mut c)?;
        if !st.net.allows_http() {
            return Ok(ERR_PERM);
        }
        let parsed = match appnet::parse_url(&bytes) {
            Ok(u) => u,
            Err(appnet::NetError::TooLong) => return Ok(ERR_INVAL),
            Err(_) => return Ok(ERR_NET),
        };
        if !st.limiter.allow(mono_ms()) {
            return Ok(ERR_NET);
        }
        crate::serial_println!(
            "[app {}] net_http_get {}://{}:{}{} (policy ok)",
            st.id,
            if parsed.https { "https" } else { "http" },
            parsed.host,
            parsed.port,
            parsed.path
        );
        // Policy passed. The transport (the shared `fetch` worker, owned by the
        // network front) cannot be driven from `appd` without racing the browser
        // for its one request slot, so the request is not sent in this build.
        Ok(ERR_NOSYS)
    });
    Ok(())
}

fn res(r: Result<i32, osjeff_core::appfs::FsError>) -> i32 {
    match r {
        Ok(v) => v,
        Err(e) => e.code(),
    }
}

/// Logs path-escape attempts once each (the sandbox counts them).
fn report_escapes(st: &mut V2) {
    let n = st.sandbox.escapes;
    if n > st.escapes_logged {
        crate::serial_println!(
            "[app {}] sandbox: path escape refused (attempt {})",
            st.id,
            n
        );
        st.escapes_logged = n;
    }
}

fn path_op(
    c: &mut C,
    p: i32,
    n: i32,
    f: impl FnOnce(
        &mut Sandbox,
        &mut dyn osjeff_core::appfs::AppFs,
        &[u8],
    ) -> Result<(), osjeff_core::appfs::FsError>,
) -> R<i32> {
    let path = match read_capped(c, p, n, MAX_PATH_ARG)? {
        Ok(b) => b,
        Err(e) => return Ok(e),
    };
    let st = v2(c)?;
    let r = appfs_backend::try_with(|fs| f(&mut st.sandbox, fs, &path));
    report_escapes(st);
    Ok(res(r.map(|()| 0)))
}

// ------------------------------------------------------------------ pixels

/// Byte offset of pixel `(x, y)` in the surface, or `None` outside it.
fn offset(info: &bootloader_api::info::FrameBufferInfo, x: i64, y: i64) -> Option<usize> {
    if x < 0 || y < 0 || x >= info.width as i64 || y >= info.height as i64 {
        return None;
    }
    Some((y as usize * info.stride + x as usize) * info.bytes_per_pixel)
}

/// Writes one RGB pixel in the surface's native order.
#[inline]
fn put_px(
    buf: &mut [u8],
    info: &bootloader_api::info::FrameBufferInfo,
    o: usize,
    r: u8,
    g: u8,
    b: u8,
) {
    use bootloader_api::info::PixelFormat;
    let bpp = info.bytes_per_pixel;
    if o + bpp > buf.len() || bpp < 3 {
        return;
    }
    match info.pixel_format {
        PixelFormat::Bgr => {
            buf[o] = b;
            buf[o + 1] = g;
            buf[o + 2] = r;
        }
        _ => {
            buf[o] = r;
            buf[o + 1] = g;
            buf[o + 2] = b;
        }
    }
}

/// `osj.blit_rgba`: 1:1 copy of an RGBA image (alpha ignored), clipped to the surface.
fn blit_1to1(st: &mut HostState, px: &[u8], w: i32, h: i32, dx: i32, dy: i32) {
    let Some(info) = st.info else { return };
    if st.fb.is_null() {
        return;
    }
    // SAFETY: `fb`/`fb_len` describe the live back surface set by `appd` right before this guest call
    // (see `HostState::set_surface`); only the `appd` thread, which is running this call, touches it.
    let buf = unsafe { core::slice::from_raw_parts_mut(st.fb, st.fb_len) };
    for row in 0..h as i64 {
        let y = dy as i64 + row;
        if y < 0 || y >= st.ch as i64 {
            continue;
        }
        for col in 0..w as i64 {
            let x = dx as i64 + col;
            if x < 0 || x >= st.cw as i64 {
                continue;
            }
            let Some(o) = offset(&info, x, y) else {
                continue;
            };
            let s = ((row * w as i64 + col) * 4) as usize;
            put_px(buf, &info, o, px[s], px[s + 1], px[s + 2]);
        }
    }
}

/// `osj.draw_image_png`: alpha-blended draw, clipped to the surface.
fn blit_alpha(st: &HostState, px: &[u8], w: i32, h: i32, dx: i32, dy: i32) {
    let Some(info) = st.info else { return };
    if st.fb.is_null() {
        return;
    }
    // SAFETY: same contract as `blit_1to1` (the `appd` thread is the only one touching the surface).
    let buf = unsafe { core::slice::from_raw_parts_mut(st.fb, st.fb_len) };
    for row in 0..h as i64 {
        let y = dy as i64 + row;
        if y < 0 || y >= st.ch as i64 {
            continue;
        }
        for col in 0..w as i64 {
            let x = dx as i64 + col;
            if x < 0 || x >= st.cw as i64 {
                continue;
            }
            let s = ((row * w as i64 + col) * 4) as usize;
            let a = px[s + 3] as u32;
            if a == 0 {
                continue;
            }
            let Some(o) = offset(&info, x, y) else {
                continue;
            };
            if a == 255 {
                put_px(buf, &info, o, px[s], px[s + 1], px[s + 2]);
                continue;
            }
            // read back the destination in RGB order
            use bootloader_api::info::PixelFormat;
            let (dr, dg, db) = match info.pixel_format {
                PixelFormat::Bgr => (buf[o + 2], buf[o + 1], buf[o]),
                _ => (buf[o], buf[o + 1], buf[o + 2]),
            };
            let mix = |s: u8, d: u8| ((s as u32 * a + d as u32 * (255 - a)) / 255) as u8;
            put_px(
                buf,
                &info,
                o,
                mix(px[s], dr),
                mix(px[s + 1], dg),
                mix(px[s + 2], db),
            );
        }
    }
}
