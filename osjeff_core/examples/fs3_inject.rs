//! Host tool: put files into an OJFS v3 disk image (the disk QEMU gets as the
//! secondary IDE master). Formats a blank image, migrates a v2 one, mounts a v3 one.
//!
//! ```text
//! cargo run -p osjeff_core --example fs3_inject -- <disk.img> <host-file> <dest-path>
//! cargo run -p osjeff_core --example fs3_inject -- <disk.img> --mkdir <dest-dir>
//! cargo run -p osjeff_core --example fs3_inject -- <disk.img> --files <count> <dest-dir>
//! cargo run -p osjeff_core --example fs3_inject -- <disk.img> --ls <dir>
//! ```
//!
//! `<dest-path>` is absolute (`/Documentos/foto.png`); missing parent folders are
//! created. `--files` makes `<count>` small files `arquivo0001.txt`... for big-folder
//! tests. Refuses an image that holds something that is not OJFS (never reformats).

use osjeff_core::blockdev::{BlockDevice, IoError, SECTOR_SIZE};
use osjeff_core::fs3::{self, Detected, FormatOptions, Fs3};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::process::ExitCode;

/// A disk image file as a block device.
struct FileDisk {
    f: File,
    sectors: u64,
}

impl FileDisk {
    fn open(path: &str) -> std::io::Result<Self> {
        let f = OpenOptions::new().read(true).write(true).open(path)?;
        let sectors = f.metadata()?.len() / SECTOR_SIZE as u64;
        Ok(FileDisk { f, sectors })
    }
}

impl BlockDevice for FileDisk {
    fn sector_count(&self) -> u64 {
        self.sectors
    }
    fn read_sectors(&mut self, lba: u64, buf: &mut [u8]) -> Result<(), IoError> {
        if lba + (buf.len() / SECTOR_SIZE) as u64 > self.sectors {
            return Err(IoError::OutOfRange);
        }
        self.f
            .seek(SeekFrom::Start(lba * SECTOR_SIZE as u64))
            .and_then(|_| self.f.read_exact(buf))
            .map_err(|_| IoError::Read)
    }
    fn write_sectors(&mut self, lba: u64, buf: &[u8]) -> Result<(), IoError> {
        if lba + (buf.len() / SECTOR_SIZE) as u64 > self.sectors {
            return Err(IoError::OutOfRange);
        }
        self.f
            .seek(SeekFrom::Start(lba * SECTOR_SIZE as u64))
            .and_then(|_| self.f.write_all(buf))
            .map_err(|_| IoError::Write)
    }
    fn flush(&mut self) -> Result<(), IoError> {
        self.f.sync_data().map_err(|_| IoError::Flush)
    }
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn mount(path: &str) -> Result<Fs3<FileDisk>, String> {
    let mut dev = FileDisk::open(path).map_err(|e| format!("{path}: {e}"))?;
    let opts = FormatOptions::new(*b"fs3_inject------", now());
    match fs3::detect(&mut dev).map_err(|e| format!("detect: {e:?}"))? {
        Detected::V3 => {}
        Detected::Blank => {
            let fs = Fs3::format(dev, &opts).map_err(|e| format!("format: {e:?}"))?;
            return Ok(fs);
        }
        Detected::V2 => {
            let img = fs3::read_v2_image(&mut dev).map_err(|e| format!("v2: {e:?}"))?;
            fs3::migrate_v2(&mut dev, &img, &opts).map_err(|e| format!("migrate: {e:?}"))?;
        }
        Detected::Unknown => return Err("the image holds unknown data: not touching it".into()),
    }
    Fs3::mount(dev).map_err(|e| format!("mount: {e:?}"))
}

/// `mkdir -p`.
fn mkdir_p(fs: &mut Fs3<FileDisk>, dir: &str) -> Result<(), String> {
    let mut path = String::new();
    for part in dir.split('/').filter(|p| !p.is_empty()) {
        path.push('/');
        path.push_str(part);
        if fs.stat(path.as_str()).is_err() {
            fs.mkdir(path.as_str(), now())
                .map_err(|e| format!("mkdir {path}: {e:?}"))?;
        }
    }
    Ok(())
}

fn run(args: &[String]) -> Result<(), String> {
    let [img, rest @ ..] = args else {
        return Err("usage: fs3_inject <disk.img> <host-file> <dest-path> | --mkdir <dir> | --files <n> <dir> | --ls <dir>".into());
    };
    let mut fs = mount(img)?;
    match rest {
        [flag, dir] if flag == "--mkdir" => mkdir_p(&mut fs, dir)?,
        [flag, dir] if flag == "--ls" => {
            for e in fs.readdir(dir.as_str()).map_err(|e| format!("{e:?}"))? {
                println!(
                    "{:>12}  {:?}  {}",
                    e.size,
                    e.kind,
                    String::from_utf8_lossy(&e.name)
                );
            }
        }
        [flag, n, dir] if flag == "--files" => {
            let n: u32 = n.parse().map_err(|_| "bad count".to_string())?;
            mkdir_p(&mut fs, dir)?;
            for i in 1..=n {
                let p = format!("{dir}/arquivo{i:04}.txt");
                let body = format!("Arquivo de teste numero {i}\n");
                fs.write_file(p.as_str(), body.as_bytes(), now())
                    .map_err(|e| format!("{p}: {e:?}"))?;
            }
            println!("wrote {n} files in {dir}");
        }
        [src, dest] => {
            let mut f = File::open(src).map_err(|e| format!("{src}: {e}"))?;
            let parent = dest
                .rsplit_once('/')
                .map_or("/", |(p, _)| if p.is_empty() { "/" } else { p });
            mkdir_p(&mut fs, parent)?;
            let _ = fs.remove(dest.as_str());
            let ino = fs
                .create(dest.as_str(), now())
                .map_err(|e| format!("create {dest}: {e:?}"))?;
            let mut buf = vec![0u8; 1 << 20];
            let mut off = 0u64;
            loop {
                let n = f.read(&mut buf).map_err(|e| e.to_string())?;
                if n == 0 {
                    break;
                }
                fs.write_at(ino, off, &buf[..n], now())
                    .map_err(|e| format!("write {dest}: {e:?}"))?;
                off += n as u64;
            }
            println!("{src} -> {dest} ({off} bytes)");
        }
        _ => return Err("bad arguments (see the header of fs3_inject.rs)".into()),
    }
    fs.sync().map_err(|e| format!("sync: {e:?}"))?;
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("fs3_inject: {e}");
            ExitCode::FAILURE
        }
    }
}
