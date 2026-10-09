//! OJFS v3 micro-benchmark over a `RamDisk`.
//!
//! ```text
//! cargo run --release -p kitsune_core --example ojfs3_bench
//! ```
//!
//! Reports host time (RAM disk, so this is the CPU cost of the format logic,
//! not of a real disk), and, more usefully, how many 512-byte sectors each
//! operation reads and writes: the write amplification the journal costs.

use kitsune_core::blockdev::{IoCounters, RamDisk};
use kitsune_core::fs3::{FormatOptions, Fs3};
use std::time::Instant;

const MIB: usize = 1024 * 1024;

fn fresh(mib: u64, inodes: u32) -> Fs3<RamDisk> {
    let mut o = FormatOptions::new([1; 16], 1_700_000_000);
    o.inode_count = Some(inodes);
    Fs3::format(RamDisk::new(mib * 2048), &o).unwrap()
}

fn delta(a: IoCounters, b: IoCounters) -> IoCounters {
    IoCounters {
        read_calls: b.read_calls - a.read_calls,
        write_calls: b.write_calls - a.write_calls,
        sectors_read: b.sectors_read - a.sectors_read,
        sectors_written: b.sectors_written - a.sectors_written,
        flushes: b.flushes - a.flushes,
    }
}

/// Run `f` and return the I/O it caused on the device.
fn io(fs: &mut Fs3<RamDisk>, f: impl FnOnce(&mut Fs3<RamDisk>)) -> IoCounters {
    let a = fs.device().counters();
    f(fs);
    delta(a, fs.device().counters())
}

fn main() {
    println!("OJFS v3 benchmark (RamDisk, host CPU; release build recommended)\n");

    // ---- sequential 8 MiB ----
    let mut fs = fresh(64, 1024);
    let data: Vec<u8> = (0..8 * MIB).map(|i| (i * 31 + i / 4096) as u8).collect();
    let t = Instant::now();
    let c = io(&mut fs, |fs| fs.write_file("/seq", &data, 1).unwrap());
    let w = t.elapsed();
    println!(
        "write 8 MiB in one call : {:>8.1} MiB/s   ({} sectors written for {} payload sectors, x{:.3})",
        8.0 / w.as_secs_f64(),
        c.sectors_written,
        8 * MIB / 512,
        c.sectors_written as f64 / (8 * MIB / 512) as f64
    );
    let ino = fs.lookup("/seq").unwrap();
    let mut buf = vec![0u8; 64 * 1024];
    let t = Instant::now();
    let mut off = 0u64;
    let c = io(&mut fs, |fs| {
        loop {
            let n = fs.read_at(ino, off, &mut buf).unwrap();
            if n == 0 {
                break;
            }
            off += n as u64;
        }
    });
    let r = t.elapsed();
    println!(
        "read 8 MiB in 64 KiB    : {:>8.1} MiB/s   ({} sectors read)",
        8.0 / r.as_secs_f64(),
        c.sectors_read
    );
    let t = Instant::now();
    let got = fs.read_file("/seq").unwrap();
    println!(
        "read 8 MiB whole        : {:>8.1} MiB/s",
        8.0 / t.elapsed().as_secs_f64()
    );
    assert_eq!(got, data);

    // 8 MiB appended in 4 KiB steps (one transaction each).
    let ino = fs.create("/app", 2).unwrap();
    let t = Instant::now();
    let c = io(&mut fs, |fs| {
        for i in 0..2048 {
            fs.append(ino, &data[i * 4096..(i + 1) * 4096], 2).unwrap();
        }
    });
    let a = t.elapsed();
    println!(
        "append 8 MiB in 4 KiB   : {:>8.1} MiB/s   ({:.1} sectors written per 8-sector append, x{:.2})",
        8.0 / a.as_secs_f64(),
        c.sectors_written as f64 / 2048.0,
        c.sectors_written as f64 / 2048.0 / 8.0
    );
    drop(fs);

    // ---- 1000 files ----
    let mut fs = fresh(64, 4096);
    fs.mkdir("/d", 1).unwrap();
    let t = Instant::now();
    let c = io(&mut fs, |fs| {
        for i in 0..1000 {
            fs.create(&format!("/d/file-{i:04}.txt"), 1).unwrap();
        }
    });
    let e = t.elapsed();
    println!(
        "\ncreate 1000 empty files : {:>8.0} files/s ({:.1} sectors written, {:.1} read, {:.1} flushes per create)",
        1000.0 / e.as_secs_f64(),
        c.sectors_written as f64 / 1000.0,
        c.sectors_read as f64 / 1000.0,
        c.flushes as f64 / 1000.0
    );
    let t = Instant::now();
    let c = io(&mut fs, |fs| {
        for i in 0..1000 {
            fs.write_file(&format!("/d/small-{i:04}.txt"), &[b'x'; 100], 2)
                .unwrap();
        }
    });
    let e = t.elapsed();
    println!(
        "write 1000 x 100 B files: {:>8.0} files/s ({:.1} sectors written per file for 100 payload bytes)",
        1000.0 / e.as_secs_f64(),
        c.sectors_written as f64 / 1000.0
    );
    let t = Instant::now();
    for i in 0..1000 {
        fs.lookup(&format!("/d/small-{i:04}.txt")).unwrap();
    }
    println!(
        "lookup in a 2000-entry directory: {:>6.0} us each",
        t.elapsed().as_secs_f64() * 1e6 / 1000.0
    );
    let c = io(&mut fs, |fs| {
        for i in 0..1000 {
            fs.remove(&format!("/d/file-{i:04}.txt")).unwrap();
        }
    });
    println!(
        "remove 1000 files       : {:.1} sectors written per remove",
        c.sectors_written as f64 / 1000.0
    );
    assert!(fs.fsck().unwrap().is_clean());

    // ---- sectors per operation ----
    println!("\nsectors per operation (read / written / flushes), fresh 8 MiB filesystem:");
    let mut fs = fresh(8, 256);
    fs.mkdir("/dir", 1).unwrap();
    fs.write_file("/dir/f", &[1u8; 5000], 1).unwrap();
    fs.reset_cache_stats();
    let rows: Vec<(&str, IoCounters)> = vec![
        (
            "create /dir/new",
            io(&mut fs, |f| {
                let _ = f.create("/dir/new", 2).unwrap();
            }),
        ),
        (
            "mkdir /dir/sub",
            io(&mut fs, |f| {
                let _ = f.mkdir("/dir/sub", 2).unwrap();
            }),
        ),
        (
            "write_file 100 B",
            io(&mut fs, |f| f.write_file("/dir/s", &[1u8; 100], 2).unwrap()),
        ),
        (
            "write_file 4 KiB",
            io(&mut fs, |f| {
                f.write_file("/dir/k", &[1u8; 4096], 2).unwrap()
            }),
        ),
        (
            "write_file 64 KiB",
            io(&mut fs, |f| {
                f.write_file("/dir/m", &[1u8; 65536], 2).unwrap()
            }),
        ),
        ("overwrite 1 KiB inside", {
            let i = fs.lookup("/dir/m").unwrap();
            io(&mut fs, |f| f.write_at(i, 4000, &[2u8; 1024], 3).unwrap())
        }),
        (
            "rename /dir/s -> /dir/t",
            io(&mut fs, |f| f.rename("/dir/s", "/dir/t", 3).unwrap()),
        ),
        (
            "trash /dir/t",
            io(&mut fs, |f| f.trash("/dir/t", 3).unwrap()),
        ),
        (
            "restore t",
            io(&mut fs, |f| drop(f.trash_restore(b"t", 4).unwrap())),
        ),
        (
            "remove /dir/t",
            io(&mut fs, |f| f.remove("/dir/t").unwrap()),
        ),
        ("read_file 64 KiB (cold)", {
            fs.sync().unwrap();
            io(&mut fs, |f| drop(f.read_file("/dir/m").unwrap()))
        }),
    ];
    println!(
        "  {:<26} {:>6} {:>8} {:>8}",
        "operation", "read", "written", "flushes"
    );
    for (name, c) in rows {
        println!(
            "  {:<26} {:>6} {:>8} {:>8}",
            name, c.sectors_read, c.sectors_written, c.flushes
        );
    }
    let s = fs.cache_stats();
    println!(
        "\nblock cache: {} hits, {} misses, {} evictions",
        s.hits, s.misses, s.evictions
    );
}
