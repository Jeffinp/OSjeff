//! Fuzz target: the OJFS on-disk format (`osjeff_core::fs`).
//!
//! The "disk" is attacker/corruption controlled, so the image is built in one of
//! three ways (see [`Image`]): fully raw bytes, raw bytes of the wrong length,
//! or a *structured* image with a valid `OJF2` magic and up to 48 records whose
//! header fields (state, flags, parent, name_len, size) are fuzzed directly, so
//! the fuzzer reaches parent cycles, oversized `size`, `name_len` > 16, etc.
//! without having to guess the 1044-byte record stride.
//!
//! Every accessor is run over every slot, then a fuzzed sequence of mutating
//! operations (write/mkdir/remove/trash/restore/purge/...) is applied.
#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use osjeff_core::fs::{self, IMAGE_SIZE, MAX_FILES, MAX_FILE_SIZE, MAX_NAME};

/// Byte offset of record `i` (4-byte magic + i * (header 22 + payload 1024)).
const REC_SIZE: usize = 1 + 1 + 1 + 1 + MAX_NAME + 2 + MAX_FILE_SIZE;

#[derive(Arbitrary, Debug)]
struct Rec {
    state: u8,
    flags: u8,
    parent: u8,
    name_len: u8,
    name: [u8; MAX_NAME],
    size: u16,
    data: Vec<u8>,
}

#[derive(Arbitrary, Debug)]
enum Image {
    /// Arbitrary bytes copied over a zeroed, correctly sized image.
    Raw(Vec<u8>),
    /// Arbitrary bytes used as-is (length is NOT IMAGE_SIZE).
    Short(Vec<u8>),
    /// Valid magic + fuzzed record headers.
    Structured { recs: Vec<Rec> },
}

#[derive(Arbitrary, Debug)]
enum Op {
    Write { parent: u8, name: Vec<u8>, data: Vec<u8> },
    Mkdir { parent: u8, name: Vec<u8> },
    Remove(Vec<u8>),
    Read(Vec<u8>),
    Trash(Vec<u8>),
    Restore(Vec<u8>),
    Purge(Vec<u8>),
    EmptyTrash,
    TrashSlot(u8),
    RestoreSlot(u8),
    PurgeSlot(u8),
    FindIn { parent: u8, name: Vec<u8> },
    Format,
}

#[derive(Arbitrary, Debug)]
struct Input {
    image: Image,
    ops: Vec<Op>,
}

fn build(image: &Image) -> Vec<u8> {
    match image {
        Image::Raw(b) => {
            let mut img = vec![0u8; IMAGE_SIZE];
            let n = b.len().min(IMAGE_SIZE);
            img[..n].copy_from_slice(&b[..n]);
            img
        }
        Image::Short(b) => b.clone(),
        Image::Structured { recs } => {
            let mut img = vec![0u8; IMAGE_SIZE];
            img[..4].copy_from_slice(b"OJF2");
            for (i, r) in recs.iter().take(MAX_FILES).enumerate() {
                let o = 4 + i * REC_SIZE;
                img[o] = r.state;
                img[o + 1] = r.flags;
                img[o + 2] = r.parent;
                img[o + 3] = r.name_len;
                img[o + 4..o + 4 + MAX_NAME].copy_from_slice(&r.name);
                img[o + 20..o + 22].copy_from_slice(&r.size.to_le_bytes());
                let d = r.data.len().min(MAX_FILE_SIZE);
                img[o + 22..o + 22 + d].copy_from_slice(&r.data[..d]);
            }
            img
        }
    }
}

/// Exercise every read-only accessor over every slot (and a few out-of-range
/// ones), asserting the documented contracts.
fn walk(img: &[u8]) {
    let _ = fs::is_formatted(img);
    let _ = fs::count(img);
    let _ = fs::count_active(img);
    let _ = fs::count_trashed(img);
    for i in 0..MAX_FILES + 3 {
        let _ = fs::is_used(img, i);
        let _ = fs::is_active(img, i);
        let _ = fs::is_trashed(img, i);
        let _ = fs::is_dir(img, i);
        let _ = fs::parent_at(img, i);
        let name = fs::name_at(img, i);
        assert!(name.len() <= MAX_NAME, "name_at longer than MAX_NAME");
        let size = fs::size_at(img, i);
        assert!(size <= MAX_FILE_SIZE, "size_at {size} > MAX_FILE_SIZE");
        if let Some(d) = fs::read_slot(img, i) {
            assert!(d.len() <= MAX_FILE_SIZE, "read_slot {} > MAX_FILE_SIZE", d.len());
        }
        let _ = fs::find_in(img, fs::parent_at(img, i), name);
        let _ = fs::find(img, name);
        let _ = fs::read(img, name);
    }
    // The file manager lists children of every directory and walks `..`
    // chains; make sure a parent cycle cannot make that unbounded here.
    for i in 0..MAX_FILES {
        let mut cur = i as u8;
        let mut steps = 0;
        while cur != fs::ROOT && steps <= MAX_FILES {
            cur = fs::parent_at(img, cur as usize);
            steps += 1;
        }
    }
}

fn apply(img: &mut [u8], op: &Op) {
    match op {
        Op::Write { parent, name, data } => {
            let _ = fs::write_in(img, *parent, name, data);
        }
        Op::Mkdir { parent, name } => {
            let _ = fs::mkdir(img, *parent, name);
        }
        Op::Remove(n) => {
            let _ = fs::remove(img, n);
        }
        Op::Read(n) => {
            let _ = fs::read(img, n);
        }
        Op::Trash(n) => {
            let _ = fs::trash(img, n);
        }
        Op::Restore(n) => {
            let _ = fs::restore(img, n);
        }
        Op::Purge(n) => {
            let _ = fs::purge(img, n);
        }
        Op::EmptyTrash => fs::empty_trash(img),
        Op::TrashSlot(i) => fs::trash_slot(img, *i as usize),
        Op::RestoreSlot(i) => fs::restore_slot(img, *i as usize),
        Op::PurgeSlot(i) => fs::purge_slot(img, *i as usize),
        Op::FindIn { parent, name } => {
            let _ = fs::find_in(img, *parent, name);
        }
        Op::Format => {
            if img.len() >= IMAGE_SIZE {
                fs::format(img);
            }
        }
    }
}

fuzz_target!(|input: Input| {
    let mut img = build(&input.image);
    walk(&img);
    for op in input.ops.iter().take(64) {
        apply(&mut img, op);
        walk(&img);
    }
});
