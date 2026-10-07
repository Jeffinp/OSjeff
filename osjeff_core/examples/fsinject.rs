//! Put a file into an OJFS v2 disk image from the host (test helper: the guest
//! has no other way to receive a file, and a v2 file is at most 1024 bytes).
//!
//! ```text
//! cargo run -p osjeff_core --example fsinject -- IMG [FOLDER/]NAME FILE
//! ```
//!
//! `IMG` is the raw disk QEMU attaches as the second IDE disk
//! (`tools/qemu-headless.sh` makes `<outdir>/fs.img`, 64 KiB of zeros). It is
//! created or formatted when blank. `NAME` is the name inside the guest (with an
//! optional one-level folder, created if missing). The settings app's wallpaper
//! picker then finds it by that path, e.g. `papel.png` or `fotos/praia.png`.

use osjeff_core::fs;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [img_path, name, file] = args.as_slice() else {
        eprintln!("usage: fsinject IMG [FOLDER/]NAME FILE");
        return ExitCode::from(2);
    };
    let data = match std::fs::read(file) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("fsinject: cannot read {file}: {e}");
            return ExitCode::FAILURE;
        }
    };
    // Disks are whole 512-byte sectors; keep at least the 64 KiB the harness uses.
    let size = fs::IMAGE_SIZE.div_ceil(512).max(128) * 512;
    let mut img = std::fs::read(img_path).unwrap_or_default();
    if img.len() < size {
        img.resize(size, 0);
    }
    if !fs::is_formatted(&img) {
        fs::format(&mut img);
    }
    let (parent, leaf) = match name.rsplit_once('/') {
        Some((dir, leaf)) => {
            let slot = match fs::find_in(&img, fs::ROOT, dir.as_bytes()) {
                Some(s) => s,
                None => match fs::mkdir(&mut img, fs::ROOT, dir.as_bytes()) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("fsinject: cannot create folder {dir}: {e:?}");
                        return ExitCode::FAILURE;
                    }
                },
            };
            (slot as u8, leaf)
        }
        None => (fs::ROOT, name.as_str()),
    };
    if let Err(e) = fs::write_in(&mut img, parent, leaf.as_bytes(), &data) {
        eprintln!(
            "fsinject: cannot write {name} ({} bytes, limit {}): {e:?}",
            data.len(),
            fs::MAX_FILE_SIZE
        );
        return ExitCode::FAILURE;
    }
    if let Err(e) = std::fs::write(img_path, &img) {
        eprintln!("fsinject: cannot write {img_path}: {e}");
        return ExitCode::FAILURE;
    }
    println!("{name}: {} bytes -> {img_path}", data.len());
    ExitCode::SUCCESS
}
