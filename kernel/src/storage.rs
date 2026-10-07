//! Storage service: OJFS v3 mounted on the filesystem disk (ATA secondary master).
//!
//! [`init`] runs once at boot (after the ATA probe, before the desktop) and decides
//! what to do with the disk from what [`fs3::detect`] finds on it:
//!
//! | Disk                                   | Action                                              |
//! |----------------------------------------|-----------------------------------------------------|
//! | no drive                               | nothing (RAM-only desktop, as before)               |
//! | smaller than 1 MiB (the old 64 KiB one) | log `TooSmall`, stay on OJFS v2, write nothing      |
//! | valid v3                               | mount (journal replay), `fsck`, expose              |
//! | v2 image in sectors 0..99              | `migrate_v2` into the v3 area, mount, expose        |
//! | blank, >= 1 MiB                        | `format` v3 + welcome files; v2 is left to the desktop, which formats it as before |
//! | `OJF3` magic with a bad checksum, or unknown content | log and **write nothing** |
//!
//! The desktop, terminal and editor still use the in-RAM v2 image (`fs::*`); until
//! their migration lands, the v3 volume is a snapshot taken at migration time and
//! the v2 image stays the desktop's source of truth. Sectors 0..127 are never
//! written by this module (the v3 library guarantees it), so the v2 image is
//! byte-for-byte what the old boot left there.
//!
//! API for the future consumers: [`with_fs`] (run a closure on the mounted
//! [`Fs3`]), [`is_v3`], [`state`] and [`now`] (Unix seconds, UTC, for the
//! timestamps v3 wants). The filesystem sits behind a [`YieldMutex`], which a
//! thread may hold across disk I/O (the ATA driver yields between sectors).

use crate::ata::AtaDisk;
use crate::sync::YieldMutex;
use core::sync::atomic::{AtomicU8, Ordering};
use osjeff_core::blockdev::BlockDevice;
use osjeff_core::fs3::{
    self, Detected, FormatOptions, Fs3, FsError, MIN_DISK_SECTORS, MigrateError,
};

/// The mounted v3 filesystem, if there is one.
static FS: YieldMutex<Option<Fs3<AtaDisk>>> = YieldMutex::new(None);

/// What [`init`] ended up with. `V3` is the only state in which [`with_fs`] works.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// [`init`] has not run yet.
    Uninit,
    /// No ATA drive on the filesystem channel.
    NoDisk,
    /// The disk is below the 1 MiB OJFS v3 needs (the old 64 KiB image): v2 only.
    TooSmall,
    /// The disk holds something this module will not touch (a damaged v3, or
    /// unrecognized data). Nothing was written.
    Unknown,
    /// An I/O or consistency error stopped the setup; nothing is exposed.
    Failed,
    /// OJFS v3 is mounted and verified.
    V3,
}

static STATE: AtomicU8 = AtomicU8::new(State::Uninit as u8);

fn set_state(s: State) {
    STATE.store(s as u8, Ordering::Release);
}

/// What the storage service found at boot.
#[allow(dead_code)] // API for the desktop/terminal/editor migration
pub fn state() -> State {
    match STATE.load(Ordering::Acquire) {
        1 => State::NoDisk,
        2 => State::TooSmall,
        3 => State::Unknown,
        4 => State::Failed,
        5 => State::V3,
        _ => State::Uninit,
    }
}

/// Whether OJFS v3 is mounted (the desktop is still on v2 until its consumers move).
#[allow(dead_code)] // API for the desktop/terminal/editor migration
pub fn is_v3() -> bool {
    state() == State::V3
}

/// Run `f` on the mounted v3 filesystem. `None` when v3 is not mounted, or the
/// lock cannot be had (re-entered from inside `f`, or its holder's thread died).
///
/// Holds a lock that other threads wait on, so keep `f` to filesystem work.
/// Every `Fs3` call is already durable when it returns `Ok`.
pub fn with_fs<R>(f: impl FnOnce(&mut Fs3<AtaDisk>) -> R) -> Option<R> {
    if !is_v3() {
        return None;
    }
    let mut guard = FS.lock().ok()?;
    guard.as_mut().map(f)
}

/// Current time in seconds since the Unix epoch, **UTC**, from the CMOS RTC (kept
/// in UTC; `rtc::TZ_OFFSET_HOURS` is only for display). 0 if the RTC is garbage.
pub fn now() -> u64 {
    crate::rtc::now_unix()
}

const MIB: u64 = 1024 * 1024;

/// Welcome files for a fresh disk. Same content as the v2 seed in
/// `desktop/mod.rs` (`Desktop::new`): keep both in sync until v2 goes away.
const SEED_README: &[u8] = b"Bem-vindo ao OSjeff.\nGerenciador de arquivos:\n setas   navegam\n Del     manda pra lixeira\n Tab     alterna arquivos/lixeira\n Enter   abre\n";
const SEED_NOTES: &[u8] = b"Arquivo de exemplo do OSjeff.";
const SEED_PROJECT: &[u8] = b"Arquivo dentro de uma pasta.";

fn format_options() -> FormatOptions {
    let now = now();
    let mut uuid = [0u8; 16];
    uuid[..8].copy_from_slice(&now.to_le_bytes());
    uuid[8..].copy_from_slice(&crate::io::rdtsc().to_le_bytes());
    FormatOptions::new(uuid, now)
}

/// Bring the storage service up. Never panics and never hangs: every failure is
/// logged and leaves the machine on the RAM/v2 path it had before.
pub fn init() {
    init_volume();
    if crate::trace::ON && option_env!("OSJ_STORAGE_SELFTEST").is_some() && is_v3() {
        selftest();
    }
}

fn init_volume() {
    let Some(mut dev) = AtaDisk::open() else {
        set_state(State::NoDisk);
        crate::serial_println!("storage: no filesystem disk");
        return;
    };
    let sectors = dev.sector_count();
    if sectors < MIN_DISK_SECTORS {
        set_state(State::TooSmall);
        crate::serial_println!(
            "storage: disk too small for OJFS v3 ({} KiB), staying on v2",
            sectors / 2
        );
        return;
    }
    match fs3::detect(&mut dev) {
        Ok(Detected::V3) => mount(dev),
        Ok(Detected::V2) => migrate(dev),
        Ok(Detected::Blank) => format_and_seed(dev),
        Ok(Detected::Unknown) => {
            set_state(State::Unknown);
            crate::serial_println!(
                "storage: disk holds an unrecognized or damaged OJFS v3, leaving it untouched"
            );
        }
        Err(e) => {
            set_state(State::Failed);
            crate::serial_println!(
                "storage: cannot read the disk ({:?}), leaving it untouched",
                e
            );
        }
    }
}

/// Mount a v3 volume, check it with `fsck` and publish it.
fn mount(dev: AtaDisk) {
    let sectors = dev.sector_count();
    match Fs3::mount(dev) {
        Ok(fs) => verify_and_install(fs, sectors),
        Err(e) => {
            set_state(State::Failed);
            crate::serial_println!("storage: OJFS v3 mount failed ({:?})", e);
        }
    }
}

/// `fsck` the freshly mounted `fs`; if clean, log the one-line result, park it in
/// the global slot and flip the state to `V3`. A dirty or unreadable volume is
/// dropped (nothing was dirty in its cache) and never exposed.
fn verify_and_install(mut fs: Fs3<AtaDisk>, sectors: u64) {
    match fs.fsck() {
        Ok(rep) if rep.is_clean() => {
            let st = fs.statfs();
            crate::serial_println!(
                "storage: OJFS v3 mounted, {} MiB, {} MiB free ({} files, {} dirs, fsck clean)",
                sectors * 512 / MIB,
                st.free_bytes() / MIB,
                rep.files,
                rep.dirs
            );
            match FS.lock() {
                Ok(mut slot) => {
                    *slot = Some(fs);
                    set_state(State::V3);
                }
                Err(_) => set_state(State::Failed),
            }
        }
        Ok(rep) => {
            set_state(State::Failed);
            crate::serial_println!(
                "storage: OJFS v3 fsck found {} problem(s), v3 left unmounted:",
                rep.total_issues
            );
            for issue in rep.issues.iter().take(4) {
                crate::serial_println!("  {:?}", issue);
            }
        }
        Err(e) => {
            set_state(State::Failed);
            crate::serial_println!("storage: OJFS v3 fsck failed ({:?}), v3 left unmounted", e);
        }
    }
}

/// A v2 image and no v3: copy it into a fresh v3 volume, then mount that.
fn migrate(mut dev: AtaDisk) {
    let img = match fs3::read_v2_image(&mut dev) {
        Ok(img) => img,
        Err(e) => {
            set_state(State::Failed);
            crate::serial_println!("storage: cannot read the v2 image ({:?}), not migrating", e);
            return;
        }
    };
    match fs3::migrate_v2(&mut dev, &img, &format_options()) {
        Ok(rep) => {
            crate::serial_println!(
                "storage: migrated OJFS v2 -> v3: {} files, {} dirs, {} in trash, {} bytes ({} renamed, {} orphans, {} skipped)",
                rep.files,
                rep.dirs,
                rep.trashed,
                rep.bytes,
                rep.renamed,
                rep.orphans,
                rep.skipped
            );
            mount(dev);
        }
        Err(MigrateError::AlreadyV3) => mount(dev),
        Err(MigrateError::TooSmall) => {
            set_state(State::TooSmall);
            crate::serial_println!(
                "storage: disk too small to migrate OJFS v2 ({} KiB), staying on v2",
                dev.sector_count() / 2
            );
        }
        Err(e) => {
            set_state(State::Failed);
            crate::serial_println!(
                "storage: OJFS v2 -> v3 migration failed ({:?}), v2 untouched",
                e
            );
        }
    }
}

/// A blank large disk: format v3 and seed the welcome files. The v2 image area is
/// not touched here: `Desktop::new` still formats and seeds v2 on its own, so the
/// current consumers keep working until they are migrated.
fn format_and_seed(dev: AtaDisk) {
    let sectors = dev.sector_count();
    let opts = format_options();
    let now = opts.now;
    let mut fs = match Fs3::format(dev, &opts) {
        Ok(fs) => fs,
        Err(e) => {
            set_state(State::Failed);
            crate::serial_println!("storage: OJFS v3 format failed ({:?})", e);
            return;
        }
    };
    match seed(&mut fs, now) {
        Ok(()) => {
            crate::serial_println!("storage: blank disk, formatted OJFS v3 with 3 welcome files")
        }
        Err(e) => crate::serial_println!("storage: welcome files not written ({:?})", e),
    }
    verify_and_install(fs, sectors);
}

fn seed(fs: &mut Fs3<AtaDisk>, now: u64) -> Result<(), FsError> {
    fs.write_file("/leiame.txt", SEED_README, now)?;
    fs.write_file("/notas.txt", SEED_NOTES, now)?;
    fs.mkdir("/Documentos", now)?;
    fs.write_file("/Documentos/projeto.txt", SEED_PROJECT, now)?;
    Ok(())
}

/// Boot-time storage self-test (perf-trace builds with `OSJ_STORAGE_SELFTEST` set at
/// compile time): create `/selftest`, write 2 MiB with a pattern, read it back and
/// compare, `fsck`, a burst of small files, delete everything, `fsck` again. Logs
/// times and MiB/s on the serial. Real ATA PIO, so under TCG it measures the whole
/// stack: cache, journal, driver and the emulated controller.
fn selftest() {
    use crate::interrupts::{TIMER_HZ, ticks};
    use alloc::vec;
    use alloc::vec::Vec;

    const SIZE: usize = 2 * 1024 * 1024;
    const SMALL: usize = 16;
    let ms = |t0: u64| (ticks() - t0) * 1000 / TIMER_HZ as u64;
    // MiB/s with one decimal, as tenths.
    let tenths = |ms: u64| (SIZE as u64 * 10_000 / ms.max(1)) / MIB;
    let data: Vec<u8> = (0..SIZE)
        .map(|i| (i as u32).wrapping_mul(2_654_435_761).to_le_bytes()[2] ^ (i >> 12) as u8)
        .collect();

    // Small-file names, shared by the burst and the cleanup.
    let small_name = |i: usize| {
        let mut name = *b"/selftest-00";
        name[10] = b'0' + (i / 10) as u8;
        name[11] = b'0' + (i % 10) as u8;
        name
    };

    let res = with_fs(|fs| -> Result<(), FsError> {
        let now = now();
        // Leftovers of an earlier run that lost power half-way are not an error.
        let _ = fs.remove("/selftest");
        for i in 0..SMALL {
            let _ = fs.remove(&small_name(i)[..]);
        }
        let t0 = ticks();
        let ino = fs.create("/selftest", now)?;
        fs.write_at(ino, 0, &data, now)?;
        fs.sync()?;
        let w = ms(t0);

        let t1 = ticks();
        let mut buf = vec![0u8; 64 * 1024];
        let mut bad = 0usize;
        let mut off = 0usize;
        while off < SIZE {
            let n = fs.read_at(ino, off as u64, &mut buf)?;
            if n == 0 {
                break;
            }
            bad += buf[..n]
                .iter()
                .zip(&data[off..off + n])
                .filter(|(a, b)| a != b)
                .count();
            off += n;
        }
        let r = ms(t1);
        crate::serial_println!(
            "storage: selftest write 2 MiB in {} ms ({}.{} MiB/s), read in {} ms ({}.{} MiB/s), {}",
            w,
            tenths(w) / 10,
            tenths(w) % 10,
            r,
            tenths(r) / 10,
            tenths(r) % 10,
            if bad == 0 && off == SIZE {
                "contents match"
            } else {
                "CONTENT MISMATCH"
            }
        );

        let t2 = ticks();
        let rep = fs.fsck()?;
        crate::serial_println!(
            "storage: selftest fsck {} in {} ms ({} files, {} dirs, {} problems)",
            if rep.is_clean() { "clean" } else { "DIRTY" },
            ms(t2),
            rep.files,
            rep.dirs,
            rep.total_issues
        );

        let t3 = ticks();
        for i in 0..SMALL {
            fs.write_file(&small_name(i)[..], b"0123456789abcdef", now)?;
        }
        let small = ms(t3);
        crate::serial_println!(
            "storage: selftest {} small files in {} ms ({} ms each)",
            SMALL,
            small,
            small / SMALL as u64
        );

        let t4 = ticks();
        fs.remove("/selftest")?;
        for i in 0..SMALL {
            fs.remove(&small_name(i)[..])?;
        }
        let rep = fs.fsck()?;
        crate::serial_println!(
            "storage: selftest removed in {} ms, fsck {} ({} free MiB)",
            ms(t4),
            if rep.is_clean() { "clean" } else { "DIRTY" },
            fs.statfs().free_bytes() / MIB
        );
        Ok(())
    });
    match res {
        Some(Ok(())) => crate::serial_println!("storage: selftest done"),
        Some(Err(e)) => crate::serial_println!("storage: selftest FAILED ({:?})", e),
        None => crate::serial_println!("storage: selftest skipped (v3 not available)"),
    }
}
