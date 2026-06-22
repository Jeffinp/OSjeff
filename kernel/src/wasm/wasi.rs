//! A minimal `wasi_snapshot_preview1` implementation — just the subset a
//! clang/wasi-libc guest like DOOM imports. The file operations serve the
//! embedded IWAD ([`super::WAD`]) as a single read-only preopened file; the rest
//! are thin stubs (one fake arg, no environment, a clock, a PRNG, stdio→serial).
//!
//! Only enough to bring a real C game up: not a conformant WASI runtime.

use super::{HostState, WAD};
use wasmi::{Caller, Linker, Memory};

// WASI errno values we use.
const OK: i32 = 0;
const BADF: i32 = 8;
const INVAL: i32 = 28;
const NOENT: i32 = 44;

// WASI filetypes.
const FT_CHAR: u8 = 2;
const FT_DIR: u8 = 3;
const FT_REG: u8 = 4;

const PREOPEN_FD: i32 = 3; // the single preopened dir "/"
const WAD_FD: i32 = 5; // the fd we hand back for the IWAD

type C<'a> = Caller<'a, HostState>;

fn mem(c: &C) -> Option<Memory> {
    super::guest_mem(c)
}
fn rd(c: &C, m: Memory, off: i32, buf: &mut [u8]) -> bool {
    m.read(c, off.max(0) as usize, buf).is_ok()
}
fn wr(c: &mut C, m: Memory, off: i32, buf: &[u8]) -> bool {
    m.write(c, off.max(0) as usize, buf).is_ok()
}
fn ru32(c: &C, m: Memory, off: i32) -> u32 {
    let mut b = [0u8; 4];
    rd(c, m, off, &mut b);
    u32::from_le_bytes(b)
}
fn wu32(c: &mut C, m: Memory, off: i32, v: u32) {
    wr(c, m, off, &v.to_le_bytes());
}
fn wu64(c: &mut C, m: Memory, off: i32, v: u64) {
    wr(c, m, off, &v.to_le_bytes());
}

/// Read up to 259 path bytes and report whether the path names our IWAD. We hold
/// the shareware `doom1.wad`, so we only answer for that basename — otherwise the
/// IWAD search (which probes doom2.wad/doom.wad first) would be served our bytes
/// under the wrong name and misidentify the game.
fn is_wad_path(c: &C, m: Memory, ptr: i32, len: i32) -> bool {
    let n = (len.max(0) as usize).min(259);
    let mut buf = [0u8; 260];
    if !rd(c, m, ptr, &mut buf[..n]) {
        return false;
    }
    let p = &buf[..n];
    let want = b"doom1.wad";
    p.len() >= want.len()
        && p[p.len() - want.len()..]
            .iter()
            .zip(want)
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
}

// ---- file ops backing the IWAD ----

fn fd_write(mut c: C, fd: i32, iovs: i32, n: i32, nwritten: i32) -> i32 {
    let Some(m) = mem(&c) else { return BADF };
    let mut total = 0u32;
    for i in 0..n {
        let base = iovs + i * 8;
        let p = ru32(&c, m, base) as i32;
        let l = ru32(&c, m, base + 4);
        total = total.wrapping_add(l);
        if fd == 1 || fd == 2 {
            // Mirror guest stdout/stderr to the serial log (capped per chunk).
            let take = (l as usize).min(512);
            let mut tmp = [0u8; 512];
            if rd(&c, m, p, &mut tmp[..take]) {
                let s = core::str::from_utf8(&tmp[..take]).unwrap_or("");
                crate::serial_print!("{}", s);
            }
        }
    }
    wu32(&mut c, m, nwritten, total);
    OK
}

fn fd_read(mut c: C, fd: i32, iovs: i32, n: i32, nread: i32) -> i32 {
    if fd != c.data().wad_fd {
        return BADF;
    }
    let Some(m) = mem(&c) else { return BADF };
    let mut pos = c.data().wad_pos;
    let mut total = 0u32;
    for i in 0..n {
        let base = iovs + i * 8;
        let p = ru32(&c, m, base) as i32;
        let l = ru32(&c, m, base + 4) as usize;
        let avail = WAD.len().saturating_sub(pos);
        let take = l.min(avail);
        if take > 0 {
            wr(&mut c, m, p, &WAD[pos..pos + take]);
            pos += take;
            total += take as u32;
        }
    }
    c.data_mut().wad_pos = pos;
    wu32(&mut c, m, nread, total);
    OK
}

fn fd_seek(mut c: C, fd: i32, offset: i64, whence: i32, newoff: i32) -> i32 {
    if fd != c.data().wad_fd {
        return BADF;
    }
    let len = WAD.len() as i64;
    let cur = c.data().wad_pos as i64;
    let base = match whence {
        0 => 0,    // SET
        1 => cur,  // CUR
        2 => len,  // END
        _ => return INVAL,
    };
    let np = (base + offset).clamp(0, len);
    c.data_mut().wad_pos = np as usize;
    let Some(m) = mem(&c) else { return BADF };
    wu64(&mut c, m, newoff, np as u64);
    OK
}

fn fd_tell(mut c: C, fd: i32, off: i32) -> i32 {
    if fd != c.data().wad_fd {
        return BADF;
    }
    let pos = c.data().wad_pos as u64;
    let Some(m) = mem(&c) else { return BADF };
    wu64(&mut c, m, off, pos);
    OK
}

fn fd_close(mut c: C, fd: i32) -> i32 {
    if fd == c.data().wad_fd {
        c.data_mut().wad_fd = -1;
    }
    OK
}

fn fd_fdstat_get(mut c: C, fd: i32, buf: i32) -> i32 {
    let Some(m) = mem(&c) else { return BADF };
    let ft = if fd == c.data().wad_fd {
        FT_REG
    } else if fd == PREOPEN_FD {
        FT_DIR
    } else if (0..=2).contains(&fd) {
        FT_CHAR
    } else {
        return BADF;
    };
    wr(&mut c, m, buf, &[ft]); // fs_filetype @0
    wu32(&mut c, m, buf + 2, 0); // fs_flags @2 (u16, write low bytes)
    wu64(&mut c, m, buf + 8, u64::MAX); // rights_base @8
    wu64(&mut c, m, buf + 16, u64::MAX); // rights_inheriting @16
    OK
}

fn fd_prestat_get(mut c: C, fd: i32, buf: i32) -> i32 {
    if fd != PREOPEN_FD {
        return BADF;
    }
    let Some(m) = mem(&c) else { return BADF };
    wr(&mut c, m, buf, &[0u8]); // tag = dir
    wu32(&mut c, m, buf + 4, 1); // pr_name_len = len("/")
    OK
}

fn fd_prestat_dir_name(mut c: C, fd: i32, path: i32, path_len: i32) -> i32 {
    if fd != PREOPEN_FD || path_len < 1 {
        return BADF;
    }
    let Some(m) = mem(&c) else { return BADF };
    wr(&mut c, m, path, b"/");
    OK
}

fn path_open(mut c: C, _dirfd: i32, _dflags: i32, path: i32, plen: i32, opened: i32) -> i32 {
    let Some(m) = mem(&c) else { return BADF };
    if !WAD.is_empty() && c.data().wad_fd < 0 && is_wad_path(&c, m, path, plen) {
        c.data_mut().wad_fd = WAD_FD;
        c.data_mut().wad_pos = 0;
        wu32(&mut c, m, opened, WAD_FD as u32);
        OK
    } else {
        NOENT
    }
}

fn path_filestat_get(mut c: C, _dirfd: i32, path: i32, plen: i32, buf: i32) -> i32 {
    let Some(m) = mem(&c) else { return BADF };
    if !WAD.is_empty() && is_wad_path(&c, m, path, plen) {
        for k in 0..8 {
            wu64(&mut c, m, buf + k * 8, 0); // zero the 64-byte filestat
        }
        wr(&mut c, m, buf + 16, &[FT_REG]); // filetype @16
        wu64(&mut c, m, buf + 32, WAD.len() as u64); // size @32
        OK
    } else {
        NOENT
    }
}

// ---- tiny stubs ----

fn clock_time_get(mut c: C, _id: i32, _prec: i64, out: i32) -> i32 {
    let ns = (crate::interrupts::ticks() * 4) * 1_000_000;
    let Some(m) = mem(&c) else { return BADF };
    wu64(&mut c, m, out, ns);
    OK
}

fn random_get(mut c: C, buf: i32, len: i32) -> i32 {
    let Some(m) = mem(&c) else { return BADF };
    let mut x = c.data().rng;
    if x == 0 {
        x = (crate::interrupts::ticks() as u32) | 1;
    }
    let mut i = 0;
    while i < len {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        wr(&mut c, m, buf + i, &[x as u8]);
        i += 1;
    }
    c.data_mut().rng = x;
    OK
}

fn args_sizes_get(mut c: C, argc: i32, buf_size: i32) -> i32 {
    let Some(m) = mem(&c) else { return BADF };
    wu32(&mut c, m, argc, 1);
    wu32(&mut c, m, buf_size, 5); // "doom\0"
    OK
}

fn args_get(mut c: C, argv: i32, argv_buf: i32) -> i32 {
    let Some(m) = mem(&c) else { return BADF };
    wr(&mut c, m, argv_buf, b"doom\0");
    wu32(&mut c, m, argv, argv_buf as u32);
    OK
}

fn environ_sizes_get(mut c: C, count: i32, buf_size: i32) -> i32 {
    let Some(m) = mem(&c) else { return BADF };
    wu32(&mut c, m, count, 0);
    wu32(&mut c, m, buf_size, 0);
    OK
}

/// Register the WASI subset on `linker` under `wasi_snapshot_preview1`.
pub(super) fn install(linker: &mut Linker<HostState>) -> Result<(), &'static str> {
    const M: &str = "wasi_snapshot_preview1";
    macro_rules! link {
        ($name:literal, $f:expr) => {
            linker.func_wrap(M, $name, $f).map_err(|_| "link wasi")?;
        };
    }

    link!("fd_write", |c: C, fd: i32, i: i32, n: i32, w: i32| fd_write(c, fd, i, n, w));
    link!("fd_read", |c: C, fd: i32, i: i32, n: i32, r: i32| fd_read(c, fd, i, n, r));
    link!("fd_seek", |c: C, fd: i32, o: i64, wh: i32, n: i32| fd_seek(c, fd, o, wh, n));
    link!("fd_tell", |c: C, fd: i32, o: i32| fd_tell(c, fd, o));
    link!("fd_close", |c: C, fd: i32| fd_close(c, fd));
    link!("fd_sync", |_c: C, _fd: i32| OK);
    link!("fd_datasync", |_c: C, _fd: i32| OK);
    link!("fd_fdstat_get", |c: C, fd: i32, b: i32| fd_fdstat_get(c, fd, b));
    link!("fd_fdstat_set_flags", |_c: C, _fd: i32, _f: i32| OK);
    link!("fd_prestat_get", |c: C, fd: i32, b: i32| fd_prestat_get(c, fd, b));
    link!("fd_prestat_dir_name", |c: C, fd: i32, p: i32, l: i32| {
        fd_prestat_dir_name(c, fd, p, l)
    });
    link!("path_open", |c: C,
                        d: i32,
                        df: i32,
                        p: i32,
                        pl: i32,
                        _of: i32,
                        _rb: i64,
                        _ri: i64,
                        _ff: i32,
                        o: i32| { path_open(c, d, df, p, pl, o) });
    link!("path_filestat_get", |c: C, d: i32, _f: i32, p: i32, pl: i32, b: i32| {
        path_filestat_get(c, d, p, pl, b)
    });
    link!("clock_time_get", |c: C, id: i32, pr: i64, o: i32| clock_time_get(c, id, pr, o));
    link!("random_get", |c: C, b: i32, l: i32| random_get(c, b, l));
    link!("args_sizes_get", |c: C, a: i32, b: i32| args_sizes_get(c, a, b));
    link!("args_get", |c: C, a: i32, b: i32| args_get(c, a, b));
    link!("environ_sizes_get", |c: C, a: i32, b: i32| environ_sizes_get(c, a, b));
    link!("environ_get", |_c: C, _a: i32, _b: i32| OK);
    // poll_oneoff: report no events fired (DOOM doesn't block on it here).
    link!("poll_oneoff", |mut c: C, _i: i32, _o: i32, _n: i32, nev: i32| {
        if let Some(m) = mem(&c) {
            wu32(&mut c, m, nev, 0);
        }
        OK
    });
    // Filesystem-mutating ops: we have no writable FS, so pretend success and
    // drop the change. DOOM only uses these for config/savegame dirs it can live
    // without; path_open for writing those returns NOENT anyway.
    link!("path_create_directory", |_c: C, _fd: i32, _p: i32, _l: i32| OK);
    link!("path_remove_directory", |_c: C, _fd: i32, _p: i32, _l: i32| OK);
    link!("path_unlink_file", |_c: C, _fd: i32, _p: i32, _l: i32| OK);
    link!("path_rename", |_c: C, _f: i32, _op: i32, _ol: i32, _nf: i32, _np: i32, _nl: i32| OK);
    // proc_exit: log and return; the guest is exiting (only on a fatal I_Error).
    link!("proc_exit", |_c: C, code: i32| {
        crate::serial_println!("wasi: proc_exit({})", code);
    });

    // Not WASI: clang lowers C `system()` to an `env.system` import. We have no
    // shell — report failure (-1); DOOM only uses it for optional niceties.
    linker
        .func_wrap("env", "system", |_c: C, _cmd: i32| -> i32 { -1 })
        .map_err(|_| "link env.system")?;

    Ok(())
}
