//! OJFS — a tiny flat filesystem over a fixed byte image.
//!
//! The kernel owns the storage (a RAM buffer now, a disk sector range later);
//! this module is the pure, unit-tested format logic that reads and edits that
//! image in place. No allocation, no hardware: every function takes the image
//! slice, so the same code is exercised on the host with a plain array.
//!
//! Layout: a 4-byte magic, then [`MAX_FILES`] fixed records:
//! `[state:1][flags:1][parent:1][name_len:1][name:16][size:2 LE][data:1024]`.
//! `flags` bit 0 marks a directory; `parent` is the slot of the containing
//! directory ([`ROOT`] = 0xFF for the top level). The tree is just records that
//! point at their parent — no separate directory blocks.

/// Magic marking a formatted image. Bumped to `OJF2` when the record layout
/// gained `flags`/`parent`, so older `OJFS` images are treated as unformatted
/// and re-formatted instead of misread.
const MAGIC: [u8; 4] = *b"OJF2";

/// Parent value for items at the top level (no containing directory).
pub const ROOT: u8 = 0xFF;

/// Maximum number of files. Bumped from 16 → 48 (the record size is unchanged,
/// so older images stay readable; the extra slots were zero/unused tail). The
/// resulting image (~49 KiB → 98 sectors) still fits a single ATA transfer.
pub const MAX_FILES: usize = 48;
/// Maximum file-name length in bytes.
pub const MAX_NAME: usize = 16;
/// Maximum file payload in bytes.
pub const MAX_FILE_SIZE: usize = 1024;

// state + flags + parent + name_len + name + size.
const HEADER: usize = 1 + 1 + 1 + 1 + MAX_NAME + 2;
const OFF_FLAGS: usize = 1;
const OFF_PARENT: usize = 2;
const OFF_NAMELEN: usize = 3;
const OFF_NAME: usize = 4;
const OFF_SIZE: usize = OFF_NAME + MAX_NAME; // 20
const FL_DIR: u8 = 1; // flags bit 0: this record is a directory
const REC_SIZE: usize = HEADER + MAX_FILE_SIZE;
/// Total image size in bytes. The backing store must be at least this big.
pub const IMAGE_SIZE: usize = 4 + MAX_FILES * REC_SIZE;

/// Why a filesystem operation failed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FsError {
    NotFormatted,
    NameTooLong,
    EmptyName,
    TooBig,
    NoSpace,
    NotFound,
}

const fn rec_off(i: usize) -> usize {
    4 + i * REC_SIZE
}

// A record's first byte is its state. Reusing it for the trash keeps the on-disk
// layout (and image size) unchanged — no reformat needed.
const ST_FREE: u8 = 0; // empty slot
const ST_ACTIVE: u8 = 1; // a live file
const ST_TRASHED: u8 = 2; // recoverable in the trash

/// Write a fresh, empty filesystem into `img` (must be at least [`IMAGE_SIZE`]).
pub fn format(img: &mut [u8]) {
    img[..4].copy_from_slice(&MAGIC);
    for i in 0..MAX_FILES {
        img[rec_off(i)] = 0; // clear the "used" flag
    }
}

/// True if `img` holds a formatted filesystem.
pub fn is_formatted(img: &[u8]) -> bool {
    img.len() >= IMAGE_SIZE && img[..4] == MAGIC
}

/// True if record slot `i` holds a file.
pub fn is_used(img: &[u8], i: usize) -> bool {
    i < MAX_FILES && img[rec_off(i)] != 0
}

/// Name bytes of slot `i` (empty if unused / out of range).
pub fn name_at(img: &[u8], i: usize) -> &[u8] {
    if !is_used(img, i) {
        return &[];
    }
    let o = rec_off(i);
    let len = img[o + OFF_NAMELEN] as usize;
    // Bounds check prevents out-of-range reads on corrupted length fields.
    &img[o + OFF_NAME..o + OFF_NAME + len.min(MAX_NAME)]
}

/// Payload size of slot `i`.
pub fn size_at(img: &[u8], i: usize) -> usize {
    if !is_used(img, i) {
        return 0;
    }
    let o = rec_off(i);
    u16::from_le_bytes([img[o + OFF_SIZE], img[o + OFF_SIZE + 1]]) as usize
}

/// True if slot `i` is a directory.
pub fn is_dir(img: &[u8], i: usize) -> bool {
    is_used(img, i) && img[rec_off(i) + OFF_FLAGS] & FL_DIR != 0
}

/// The slot of `i`'s parent directory ([`ROOT`] for a top-level item).
pub fn parent_at(img: &[u8], i: usize) -> u8 {
    if is_used(img, i) {
        img[rec_off(i) + OFF_PARENT]
    } else {
        ROOT
    }
}

/// Number of records currently stored (files + dirs, any state).
pub fn count(img: &[u8]) -> usize {
    (0..MAX_FILES).filter(|&i| is_used(img, i)).count()
}

/// Find an active item named `name` directly inside directory `parent`.
pub fn find_in(img: &[u8], parent: u8, name: &[u8]) -> Option<usize> {
    (0..MAX_FILES)
        .find(|&i| is_active(img, i) && parent_at(img, i) == parent && name_at(img, i) == name)
}

/// Find an active top-level item named `name`.
pub fn find(img: &[u8], name: &[u8]) -> Option<usize> {
    find_in(img, ROOT, name)
}

/// Read the contents of slot `i` (`None` for directories / unused).
pub fn read_slot(img: &[u8], i: usize) -> Option<&[u8]> {
    if !is_used(img, i) || is_dir(img, i) {
        return None;
    }
    let o = rec_off(i);
    let s = size_at(img, i);
    Some(&img[o + HEADER..o + HEADER + s])
}

/// Read the contents of a top-level file `name`.
pub fn read<'a>(img: &'a [u8], name: &[u8]) -> Option<&'a [u8]> {
    read_slot(img, find(img, name)?)
}

/// Allocate (or reuse a same-name) active record in `parent`, returning its slot
/// and writing its header. Shared by [`write_in`] and [`mkdir`].
fn alloc(img: &mut [u8], parent: u8, name: &[u8], dir: bool) -> Result<usize, FsError> {
    if !is_formatted(img) {
        return Err(FsError::NotFormatted);
    }
    if name.is_empty() {
        return Err(FsError::EmptyName);
    }
    if name.len() > MAX_NAME {
        return Err(FsError::NameTooLong);
    }
    let slot = find_in(img, parent, name)
        .or_else(|| (0..MAX_FILES).find(|&i| !is_used(img, i)))
        .ok_or(FsError::NoSpace)?;
    let o = rec_off(slot);
    img[o] = ST_ACTIVE;
    img[o + OFF_FLAGS] = if dir { FL_DIR } else { 0 };
    img[o + OFF_PARENT] = parent;
    img[o + OFF_NAMELEN] = name.len() as u8;
    img[o + OFF_NAME..o + OFF_NAME + name.len()].copy_from_slice(name);
    Ok(slot)
}

/// Create or overwrite file `name` inside directory `parent`.
pub fn write_in(img: &mut [u8], parent: u8, name: &[u8], data: &[u8]) -> Result<(), FsError> {
    if data.len() > MAX_FILE_SIZE {
        return Err(FsError::TooBig);
    }
    let o = rec_off(alloc(img, parent, name, false)?);
    let sz = (data.len() as u16).to_le_bytes();
    img[o + OFF_SIZE] = sz[0];
    img[o + OFF_SIZE + 1] = sz[1];
    img[o + HEADER..o + HEADER + data.len()].copy_from_slice(data);
    Ok(())
}

/// Create or overwrite a top-level file.
pub fn write(img: &mut [u8], name: &[u8], data: &[u8]) -> Result<(), FsError> {
    write_in(img, ROOT, name, data)
}

/// Create directory `name` inside `parent`. Returns its slot.
pub fn mkdir(img: &mut [u8], parent: u8, name: &[u8]) -> Result<usize, FsError> {
    let slot = alloc(img, parent, name, true)?;
    let o = rec_off(slot);
    img[o + OFF_SIZE] = 0;
    img[o + OFF_SIZE + 1] = 0;
    Ok(slot)
}

/// Permanently delete top-level `name` (recursive). `NotFound` if absent.
pub fn remove(img: &mut [u8], name: &[u8]) -> Result<(), FsError> {
    let i = find(img, name).ok_or(FsError::NotFound)?;
    purge_slot(img, i);
    Ok(())
}

// ---- trash / permanent delete ----

/// True if slot `i` holds a live (non-trashed) file.
pub fn is_active(img: &[u8], i: usize) -> bool {
    i < MAX_FILES && img[rec_off(i)] == ST_ACTIVE
}

/// True if slot `i` holds a file currently in the trash.
pub fn is_trashed(img: &[u8], i: usize) -> bool {
    i < MAX_FILES && img[rec_off(i)] == ST_TRASHED
}

/// Number of live (non-trashed) files.
pub fn count_active(img: &[u8]) -> usize {
    (0..MAX_FILES).filter(|&i| is_active(img, i)).count()
}

/// Number of files in the trash.
pub fn count_trashed(img: &[u8]) -> usize {
    (0..MAX_FILES).filter(|&i| is_trashed(img, i)).count()
}

/// Move slot `i` (and, for a directory, all its descendants) to the trash.
pub fn trash_slot(img: &mut [u8], i: usize) {
    if !is_active(img, i) {
        return;
    }
    if is_dir(img, i) {
        for c in 0..MAX_FILES {
            if is_active(img, c) && parent_at(img, c) == i as u8 {
                trash_slot(img, c);
            }
        }
    }
    img[rec_off(i)] = ST_TRASHED;
}

/// Restore slot `i` (and its trashed descendants) from the trash.
pub fn restore_slot(img: &mut [u8], i: usize) {
    if !is_trashed(img, i) {
        return;
    }
    img[rec_off(i)] = ST_ACTIVE;
    if is_dir(img, i) {
        for c in 0..MAX_FILES {
            if is_trashed(img, c) && parent_at(img, c) == i as u8 {
                restore_slot(img, c);
            }
        }
    }
}

/// Permanently delete slot `i` (and, for a directory, its whole subtree).
pub fn purge_slot(img: &mut [u8], i: usize) {
    if !is_used(img, i) {
        return;
    }
    if is_dir(img, i) {
        for c in 0..MAX_FILES {
            if is_used(img, c) && parent_at(img, c) == i as u8 {
                purge_slot(img, c);
            }
        }
    }
    img[rec_off(i)] = ST_FREE;
}

/// Move a live top-level `name` to the trash (recursive). `NotFound` if absent.
pub fn trash(img: &mut [u8], name: &[u8]) -> Result<(), FsError> {
    let i = find_in(img, ROOT, name).ok_or(FsError::NotFound)?;
    trash_slot(img, i);
    Ok(())
}

/// Restore a trashed top-level `name`. `NotFound` if not in the trash.
pub fn restore(img: &mut [u8], name: &[u8]) -> Result<(), FsError> {
    let i = (0..MAX_FILES)
        .find(|&i| is_trashed(img, i) && parent_at(img, i) == ROOT && name_at(img, i) == name)
        .ok_or(FsError::NotFound)?;
    restore_slot(img, i);
    Ok(())
}

/// Permanently delete top-level `name` (any state, recursive). Unrecoverable.
pub fn purge(img: &mut [u8], name: &[u8]) -> Result<(), FsError> {
    let i = find(img, name).ok_or(FsError::NotFound)?;
    purge_slot(img, i);
    Ok(())
}

/// Permanently delete every file in the trash.
pub fn empty_trash(img: &mut [u8]) {
    for i in 0..MAX_FILES {
        if is_trashed(img, i) {
            img[rec_off(i)] = ST_FREE;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img() -> [u8; IMAGE_SIZE] {
        let mut img = [0u8; IMAGE_SIZE];
        format(&mut img);
        img
    }

    #[test]
    fn fresh_image_is_formatted_and_empty() {
        let img = img();
        assert!(is_formatted(&img));
        assert_eq!(count(&img), 0);
    }

    #[test]
    fn unformatted_image_detected() {
        let img = [0u8; IMAGE_SIZE];
        assert!(!is_formatted(&img));
        let mut img2 = img;
        assert_eq!(write(&mut img2, b"x", b"y"), Err(FsError::NotFormatted));
    }

    #[test]
    fn write_then_read() {
        let mut img = img();
        write(&mut img, b"notes.txt", b"hello world").unwrap();
        assert_eq!(read(&img, b"notes.txt"), Some(&b"hello world"[..]));
        assert_eq!(count(&img), 1);
    }

    #[test]
    fn overwrite_reuses_slot() {
        let mut img = img();
        write(&mut img, b"a", b"first").unwrap();
        write(&mut img, b"a", b"second longer").unwrap();
        assert_eq!(read(&img, b"a"), Some(&b"second longer"[..]));
        assert_eq!(count(&img), 1);
    }

    #[test]
    fn trash_restore_purge_flow() {
        let mut img = img();
        write(&mut img, b"a.txt", b"hi").unwrap();
        write(&mut img, b"b.txt", b"yo").unwrap();
        assert_eq!(count_active(&img), 2);

        // Trash keeps the data, hides it from the active count and from read().
        trash(&mut img, b"a.txt").unwrap();
        assert_eq!(count_active(&img), 1);
        assert_eq!(count_trashed(&img), 1);
        assert_eq!(read(&img, b"a.txt"), None); // read() only sees live files now
        let ti = (0..MAX_FILES)
            .find(|&i| is_trashed(&img, i) && name_at(&img, i) == b"a.txt")
            .unwrap();
        assert_eq!(read_slot(&img, ti), Some(&b"hi"[..])); // data still there
        assert_eq!(trash(&mut img, b"a.txt"), Err(FsError::NotFound)); // already trashed

        // Restore brings it back.
        restore(&mut img, b"a.txt").unwrap();
        assert_eq!(count_active(&img), 2);
        assert_eq!(count_trashed(&img), 0);

        // Permanent delete frees the slot and the data is gone.
        purge(&mut img, b"a.txt").unwrap();
        assert_eq!(count_active(&img), 1);
        assert_eq!(read(&img, b"a.txt"), None);

        // Empty trash purges everything in it.
        trash(&mut img, b"b.txt").unwrap();
        empty_trash(&mut img);
        assert_eq!(count_trashed(&img), 0);
        assert_eq!(read(&img, b"b.txt"), None);
    }

    #[test]
    fn directories_and_recursive_delete() {
        let mut img = img();
        let docs = mkdir(&mut img, ROOT, b"docs").unwrap();
        assert!(is_dir(&img, docs));
        write_in(&mut img, docs as u8, b"a.txt", b"hi").unwrap();
        write_in(&mut img, ROOT, b"root.txt", b"r").unwrap();

        // a.txt lives inside docs, not at the root.
        assert!(find_in(&img, docs as u8, b"a.txt").is_some());
        assert!(find_in(&img, ROOT, b"a.txt").is_none());
        let child = find_in(&img, docs as u8, b"a.txt").unwrap();
        assert_eq!(read_slot(&img, child), Some(&b"hi"[..]));
        assert_eq!(read_slot(&img, docs), None); // dirs have no payload

        // Trashing the directory takes its contents with it.
        trash_slot(&mut img, docs);
        assert!(is_trashed(&img, docs) && is_trashed(&img, child));
        assert_eq!(count_active(&img), 1); // only root.txt

        // Restore brings the whole subtree back.
        restore_slot(&mut img, docs);
        assert!(is_active(&img, docs) && is_active(&img, child));

        // Permanent delete frees the whole subtree.
        purge_slot(&mut img, docs);
        assert!(!is_used(&img, docs) && !is_used(&img, child));
        assert_eq!(count_active(&img), 1);
    }

    #[test]
    fn multiple_files() {
        let mut img = img();
        write(&mut img, b"one", b"1").unwrap();
        write(&mut img, b"two", b"22").unwrap();
        write(&mut img, b"three", b"333").unwrap();
        assert_eq!(count(&img), 3);
        assert_eq!(read(&img, b"two"), Some(&b"22"[..]));
        assert_eq!(read(&img, b"missing"), None);
    }

    #[test]
    fn remove_frees_slot() {
        let mut img = img();
        write(&mut img, b"gone", b"data").unwrap();
        assert!(remove(&mut img, b"gone").is_ok());
        assert_eq!(read(&img, b"gone"), None);
        assert_eq!(count(&img), 0);
        assert_eq!(remove(&mut img, b"gone"), Err(FsError::NotFound));
    }

    #[test]
    fn rejects_bad_names_and_sizes() {
        let mut img = img();
        assert_eq!(write(&mut img, b"", b"x"), Err(FsError::EmptyName));
        let long = [b'a'; MAX_NAME + 1];
        assert_eq!(write(&mut img, &long, b"x"), Err(FsError::NameTooLong));
        let big = [0u8; MAX_FILE_SIZE + 1];
        assert_eq!(write(&mut img, b"f", &big), Err(FsError::TooBig));
    }

    #[test]
    fn no_space_when_full() {
        let mut img = img();
        for i in 0..MAX_FILES {
            let name = [b'a' + i as u8];
            write(&mut img, &name, b"x").unwrap();
        }
        assert_eq!(write(&mut img, b"overflow", b"x"), Err(FsError::NoSpace));
        // Overwriting an existing file still works when full.
        assert!(write(&mut img, b"a", b"updated").is_ok());
    }

    #[test]
    fn name_and_size_accessors() {
        let mut img = img();
        write(&mut img, b"hi", b"abcd").unwrap();
        let slot = find(&img, b"hi").unwrap();
        assert_eq!(name_at(&img, slot), b"hi");
        assert_eq!(size_at(&img, slot), 4);
    }

    #[test]
    fn max_size_file_roundtrips() {
        let mut img = img();
        let data = [b'z'; MAX_FILE_SIZE];
        write(&mut img, b"big", &data).unwrap();
        assert_eq!(read(&img, b"big"), Some(&data[..]));
    }
}
