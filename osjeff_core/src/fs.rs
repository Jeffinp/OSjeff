//! OJFS — a tiny flat filesystem over a fixed byte image.
//!
//! The kernel owns the storage (a RAM buffer now, a disk sector range later);
//! this module is the pure, unit-tested format logic that reads and edits that
//! image in place. No allocation, no hardware: every function takes the image
//! slice, so the same code is exercised on the host with a plain array.
//!
//! Layout: a 4-byte magic, then [`MAX_FILES`] fixed records:
//! `[used:1][name_len:1][name:16][size:2 LE][data:1024]`.

/// Magic marking a formatted image.
const MAGIC: [u8; 4] = *b"OJFS";

/// Maximum number of files. Bumped from 16 → 48 (the record size is unchanged,
/// so older images stay readable; the extra slots were zero/unused tail). The
/// resulting image (~49 KiB → 98 sectors) still fits a single ATA transfer.
pub const MAX_FILES: usize = 48;
/// Maximum file-name length in bytes.
pub const MAX_NAME: usize = 16;
/// Maximum file payload in bytes.
pub const MAX_FILE_SIZE: usize = 1024;

const HEADER: usize = 1 + 1 + MAX_NAME + 2; // used + name_len + name + size
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
    let len = img[o + 1] as usize;
    // Bounds check prevents out-of-range reads on corrupted length fields.
    &img[o + 2..o + 2 + len.min(MAX_NAME)]
}

/// Payload size of slot `i`.
pub fn size_at(img: &[u8], i: usize) -> usize {
    if !is_used(img, i) {
        return 0;
    }
    let o = rec_off(i);
    u16::from_le_bytes([img[o + 18], img[o + 19]]) as usize
}

/// Number of files currently stored.
pub fn count(img: &[u8]) -> usize {
    (0..MAX_FILES).filter(|&i| is_used(img, i)).count()
}

/// Find the slot holding `name`, if any.
pub fn find(img: &[u8], name: &[u8]) -> Option<usize> {
    (0..MAX_FILES).find(|&i| is_used(img, i) && name_at(img, i) == name)
}

/// Read the contents of `name`.
pub fn read<'a>(img: &'a [u8], name: &[u8]) -> Option<&'a [u8]> {
    let i = find(img, name)?;
    let o = rec_off(i);
    let s = size_at(img, i);
    Some(&img[o + HEADER..o + HEADER + s])
}

/// Create or overwrite `name` with `data`.
pub fn write(img: &mut [u8], name: &[u8], data: &[u8]) -> Result<(), FsError> {
    if !is_formatted(img) {
        return Err(FsError::NotFormatted);
    }
    if name.is_empty() {
        return Err(FsError::EmptyName);
    }
    if name.len() > MAX_NAME {
        return Err(FsError::NameTooLong);
    }
    if data.len() > MAX_FILE_SIZE {
        return Err(FsError::TooBig);
    }
    // Overwrite an existing file, else take the first free slot.
    let slot = find(img, name)
        .or_else(|| (0..MAX_FILES).find(|&i| !is_used(img, i)))
        .ok_or(FsError::NoSpace)?;

    let o = rec_off(slot);
    img[o] = ST_ACTIVE;
    img[o + 1] = name.len() as u8;
    img[o + 2..o + 2 + name.len()].copy_from_slice(name);
    let sz = (data.len() as u16).to_le_bytes();
    img[o + 18] = sz[0];
    img[o + 19] = sz[1];
    img[o + HEADER..o + HEADER + data.len()].copy_from_slice(data);
    Ok(())
}

/// Delete `name`. Returns `NotFound` if it does not exist.
pub fn remove(img: &mut [u8], name: &[u8]) -> Result<(), FsError> {
    let i = find(img, name).ok_or(FsError::NotFound)?;
    img[rec_off(i)] = ST_FREE;
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

/// Move a live `name` to the trash — recoverable with [`restore`]. The slot and
/// data are kept; only the state changes. `NotFound` if no live file matches.
pub fn trash(img: &mut [u8], name: &[u8]) -> Result<(), FsError> {
    let i = (0..MAX_FILES)
        .find(|&i| is_active(img, i) && name_at(img, i) == name)
        .ok_or(FsError::NotFound)?;
    img[rec_off(i)] = ST_TRASHED;
    Ok(())
}

/// Restore a trashed `name` back to a live file. `NotFound` if not in the trash.
pub fn restore(img: &mut [u8], name: &[u8]) -> Result<(), FsError> {
    let i = (0..MAX_FILES)
        .find(|&i| is_trashed(img, i) && name_at(img, i) == name)
        .ok_or(FsError::NotFound)?;
    img[rec_off(i)] = ST_ACTIVE;
    Ok(())
}

/// Permanently delete `name` (any state), freeing its slot. Unrecoverable.
pub fn purge(img: &mut [u8], name: &[u8]) -> Result<(), FsError> {
    let i = find(img, name).ok_or(FsError::NotFound)?;
    img[rec_off(i)] = ST_FREE;
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

        // Trash keeps the data, hides it from the active count.
        trash(&mut img, b"a.txt").unwrap();
        assert_eq!(count_active(&img), 1);
        assert_eq!(count_trashed(&img), 1);
        assert_eq!(read(&img, b"a.txt"), Some(&b"hi"[..]));
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
