//! OJFS v3: a journaled, checksummed, extent-based filesystem over a
//! [`BlockDevice`](crate::storage::blockdev::BlockDevice).
//!
//! The byte-level format, the commit protocol and the power-loss argument are
//! specified in `docs/design/ojfs3.md`; this file holds the core state
//! (superblock, bitmaps, transactions, journal replay) and the public types.
//! The namespace and data operations live in [`ops`], directories in `dir`,
//! extents in `extent`, the verifier in [`fsck`] and the v2 migration in
//! [`migrate`].
//!
//! # Atomicity in one paragraph
//!
//! Every public mutating operation is one transaction. Metadata blocks are
//! modified only in a RAM overlay ([`Tx`]); file data goes to *new* blocks
//! (copy-on-write), unreferenced until commit. `commit` writes the overlay to
//! the journal, flushes, writes the commit record (one block, checksummed over
//! itself and the payload), flushes, copies the blocks to their home
//! locations, flushes, clears the record, flushes. Mount replays a valid
//! record. So after a power cut the state is exactly "before" or "after" the
//! interrupted operation, and an operation that returned is durable.

pub mod bits;
pub mod crc32;
mod dir;
mod extent;
pub mod fsck;
pub mod inode;
pub mod layout;
pub mod migrate;
pub mod ops;

#[cfg(test)]
mod tests;

pub use fsck::{FsckIssue, FsckReport};
pub use inode::{Extent, Inode, Kind, MAX_NAME};
pub use layout::{
    FS_START_LBA, Geometry, MIN_DISK_SECTORS, ROOT_INO, Superblock, TRASH_INO, TRASH_NAME,
};
pub use migrate::{MigrateError, MigrationReport, migrate_v2, read_v2_image};

use crate::storage::blockcache::{BLOCK_SIZE, Block, BlockCache, CacheStats, SECTORS_PER_BLOCK};
use crate::storage::blockdev::{BlockDevice, IoError, SECTOR_SIZE};
use alloc::boxed::Box;
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::vec::Vec;
use crc32::crc32;
use layout::{
    BITMAP_WORDS_PER_BLOCK, BITS_PER_BITMAP_BLOCK, DIR_MAGIC, EXT_MAGIC, INODE_SIZE,
    INODES_PER_BLOCK, build_journal_header, journal_crc_ok, parse_journal_header, rd16, rd32, wr32,
};

/// An inode number.
pub type Ino = u32;

const V2_MAGIC: [u8; 4] = *b"OJF2";

/// Largest file size in bytes (logical block numbers are `u32`).
pub const MAX_FILE_BYTES: u64 = (u32::MAX as u64 - 1) * BLOCK_SIZE as u64;
/// Default number of 4 KiB blocks in the block cache.
pub const DEFAULT_CACHE_BLOCKS: usize = 128;

/// Why a filesystem call failed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FsError {
    NotFound,
    Exists,
    NotDir,
    IsDir,
    NotEmpty,
    InvalidName,
    NameTooLong,
    InvalidPath,
    /// `/.trash` (or something inside it) cannot be changed this way.
    Reserved,
    /// A directory cannot be moved into itself.
    InvalidMove,
    NoSpace,
    NoInodes,
    TooBig,
    /// The operation would touch more metadata blocks than one journal
    /// transaction holds (extreme fragmentation); nothing was changed.
    TxTooLarge,
    /// The device cannot hold a v3 filesystem (or the content being migrated).
    TooSmall,
    BadSuperblock,
    /// On-disk structure is inconsistent.
    Corrupt(&'static str),
    /// An earlier I/O error during a commit made the in-memory state
    /// untrustworthy; drop the filesystem and mount again.
    Poisoned,
    Io(IoError),
}

impl From<IoError> for FsError {
    fn from(e: IoError) -> Self {
        FsError::Io(e)
    }
}

/// What [`detect`] found at the start of a device.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Detected {
    /// A valid OJFS v3 superblock.
    V3,
    /// An OJFS v2 image at LBA 0 and no valid v3.
    V2,
    /// The first 68 KiB are all zero.
    Blank,
    /// Anything else (including a damaged v3: never reformatted automatically).
    Unknown,
}

/// Parameters of [`Fs3::format`].
#[derive(Clone, Copy, Debug)]
pub struct FormatOptions {
    /// Filesystem UUID (the caller supplies the entropy).
    pub uuid: [u8; 16],
    /// Creation time, seconds since the Unix epoch.
    pub now: u64,
    /// Inode count (default: one per 16 KiB, at least 64; rounded up to a multiple of 8).
    pub inode_count: Option<u32>,
    /// Journal payload blocks (default `clamp(total/16, 24, 256)`).
    pub journal_blocks: Option<u32>,
    /// Block-cache size in blocks (default [`DEFAULT_CACHE_BLOCKS`]).
    pub cache_blocks: Option<usize>,
}

impl FormatOptions {
    /// Defaults with the given UUID and timestamp.
    pub fn new(uuid: [u8; 16], now: u64) -> Self {
        FormatOptions {
            uuid,
            now,
            inode_count: None,
            journal_blocks: None,
            cache_blocks: None,
        }
    }
}

/// `stat` result.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Stat {
    pub ino: Ino,
    pub kind: Kind,
    pub size: u64,
    pub ctime: u64,
    pub mtime: u64,
    pub mode: u16,
    pub uid: u32,
    pub nlink: u32,
    /// Allocated data blocks (4 KiB each; holes excluded).
    pub blocks: u32,
    pub parent: Ino,
}

/// One entry returned by `readdir`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DirEntry {
    pub name: Vec<u8>,
    pub ino: Ino,
    pub kind: Kind,
    pub size: u64,
    pub mtime: u64,
}

/// One item in the trash.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TrashEntry {
    /// Name inside `/.trash` (the key for restore/purge).
    pub trash_name: Vec<u8>,
    /// Name it had where it was deleted.
    pub orig_name: Vec<u8>,
    /// Directory it was deleted from.
    pub orig_parent: Ino,
    pub ino: Ino,
    pub kind: Kind,
    pub size: u64,
    /// When it was moved to the trash.
    pub deleted_at: u64,
}

/// `statfs` result.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StatFs {
    pub block_size: u32,
    pub total_blocks: u32,
    pub free_blocks: u32,
    /// Blocks usable for data and directories (excludes the fixed metadata).
    pub data_blocks: u32,
    pub total_inodes: u32,
    pub free_inodes: u32,
}

impl StatFs {
    /// Free space in bytes.
    pub fn free_bytes(&self) -> u64 {
        self.free_blocks as u64 * self.block_size as u64
    }
    /// Capacity for data in bytes.
    pub fn data_bytes(&self) -> u64 {
        self.data_blocks as u64 * self.block_size as u64
    }
}

/// State of the transaction in progress.
struct Tx {
    active: bool,
    /// Metadata blocks modified so far (whole-block images, by block number).
    meta: BTreeMap<u32, Box<Block>>,
    touched_bb: BTreeSet<u32>,
    touched_ib: BTreeSet<u32>,
    /// Undo log for the in-RAM bitmaps: `(start, len, value_now)`.
    blog: Vec<(u32, u32, bool)>,
    ilog: Vec<(u32, bool)>,
    /// Ranges to free at commit (still marked used until then).
    to_free: Vec<(u32, u32)>,
    /// Data runs written by this transaction (discarded from the cache on abort).
    data: Vec<(u32, u32)>,
    free_blocks0: u32,
    free_inodes0: u32,
    hint0: u32,
}

impl Tx {
    fn idle() -> Tx {
        Tx {
            active: false,
            meta: BTreeMap::new(),
            touched_bb: BTreeSet::new(),
            touched_ib: BTreeSet::new(),
            blog: Vec::new(),
            ilog: Vec::new(),
            to_free: Vec::new(),
            data: Vec::new(),
            free_blocks0: 0,
            free_inodes0: 0,
            hint0: 0,
        }
    }
}

/// The filesystem. See the module documentation and `docs/design/ojfs3.md`.
pub struct Fs3<D: BlockDevice> {
    cache: BlockCache<D>,
    sb: Superblock,
    geo: Geometry,
    bbits: Vec<u64>,
    ibits: Vec<u64>,
    free_blocks: u32,
    free_inodes: u32,
    alloc_hint: u32,
    seq: u64,
    tx: Tx,
    poisoned: bool,
    commits: u64,
}

// ---------------------------------------------------------------------------
// Detection and superblock I/O
// ---------------------------------------------------------------------------

enum SbState {
    Valid(Superblock),
    /// The `OJF3` magic is there but the checksum/geometry is not.
    Damaged,
    Absent,
}

/// Total v3 blocks a device of `sectors` sectors can hold.
fn blocks_for(sectors: u64) -> u32 {
    let b = sectors.saturating_sub(FS_START_LBA) / SECTORS_PER_BLOCK;
    b.min(u32::MAX as u64) as u32
}

fn read_sb_state<D: BlockDevice>(dev: &mut D) -> Result<SbState, IoError> {
    let sectors = dev.sector_count();
    let tb = blocks_for(sectors);
    if tb == 0 {
        return Ok(SbState::Absent);
    }
    let mut damaged = false;
    let mut s = [0u8; SECTOR_SIZE];
    dev.read_sectors(FS_START_LBA, &mut s)?;
    match Superblock::decode(&s) {
        Some(sb) if sb.geo.total_blocks <= tb => return Ok(SbState::Valid(sb)),
        Some(_) => damaged = true,
        None => damaged |= s[4..8] == layout::MAGIC,
    }
    let backup = FS_START_LBA + (tb as u64 - 1) * SECTORS_PER_BLOCK;
    dev.read_sectors(backup, &mut s)?;
    match Superblock::decode(&s) {
        Some(sb) if sb.geo.total_blocks == tb => return Ok(SbState::Valid(sb)),
        Some(_) => damaged = true,
        None => damaged |= s[4..8] == layout::MAGIC,
    }
    Ok(if damaged {
        SbState::Damaged
    } else {
        SbState::Absent
    })
}

/// Classify what is on `dev` (reads at most the first 136 sectors).
pub fn detect<D: BlockDevice>(dev: &mut D) -> Result<Detected, IoError> {
    match read_sb_state(dev)? {
        SbState::Valid(_) => return Ok(Detected::V3),
        SbState::Damaged => return Ok(Detected::Unknown),
        SbState::Absent => {}
    }
    let n = dev.sector_count().min(FS_START_LBA + SECTORS_PER_BLOCK) as usize;
    if n == 0 {
        return Ok(Detected::Blank);
    }
    let mut buf = alloc::vec![0u8; n * SECTOR_SIZE];
    dev.read_sectors(0, &mut buf)?;
    if buf[..4] == V2_MAGIC {
        return Ok(Detected::V2);
    }
    if buf.iter().all(|&b| b == 0) {
        Ok(Detected::Blank)
    } else {
        Ok(Detected::Unknown)
    }
}

// ---------------------------------------------------------------------------
// Bitmap block images
// ---------------------------------------------------------------------------

fn bitmap_image(words: &[u64], idx: u32) -> Block {
    let mut b = [0u8; BLOCK_SIZE];
    let first = idx as usize * BITMAP_WORDS_PER_BLOCK as usize;
    for k in 0..BITMAP_WORDS_PER_BLOCK as usize {
        let w = words.get(first + k).copied().unwrap_or(!0u64);
        b[8 + 8 * k..16 + 8 * k].copy_from_slice(&w.to_le_bytes());
    }
    let c = crc32(&b[4..]);
    wr32(&mut b, 0, c);
    b
}

/// Check a bitmap block and copy its words into `words`.
fn load_bitmap_block(b: &[u8], idx: u32, words: &mut [u64]) -> Result<(), FsError> {
    if rd32(b, 0) != crc32(&b[4..BLOCK_SIZE]) {
        return Err(FsError::Corrupt("bitmap checksum"));
    }
    let first = idx as usize * BITMAP_WORDS_PER_BLOCK as usize;
    for k in 0..BITMAP_WORDS_PER_BLOCK as usize {
        if let Some(w) = words.get_mut(first + k) {
            let mut a = [0u8; 8];
            a.copy_from_slice(&b[8 + 8 * k..16 + 8 * k]);
            *w = u64::from_le_bytes(a);
        }
    }
    Ok(())
}

fn verify_typed(b: &Block, ext: bool, owner: u32) -> Result<(), FsError> {
    if rd32(b, 0) != crc32(&b[4..]) {
        return Err(FsError::Corrupt("block checksum"));
    }
    if ext {
        if b[4..8] != EXT_MAGIC || rd32(b, 16) != owner {
            return Err(FsError::Corrupt("extent block header"));
        }
    } else if b[4..8] != DIR_MAGIC || rd32(b, 8) != owner || rd16(b, 12) as usize > BLOCK_SIZE - 16
    {
        return Err(FsError::Corrupt("directory block header"));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Construction: format, mount
// ---------------------------------------------------------------------------

impl<D: BlockDevice> Fs3<D> {
    /// Create a new, empty filesystem on `dev` (use `&mut dev` to keep the
    /// device on failure). The v3 area starts at LBA 128; sectors 0..127 are
    /// never written. The superblock is written last.
    pub fn format(dev: D, opts: &FormatOptions) -> Result<Self, FsError> {
        let mut fs = Self::format_unsealed(dev, opts)?;
        fs.seal()?;
        Ok(fs)
    }

    /// Build the whole structure but do not write the superblock yet, so the
    /// device still reads as "no v3". Used by `format` and the migration.
    pub(crate) fn format_unsealed(dev: D, opts: &FormatOptions) -> Result<Self, FsError> {
        let sectors = dev.sector_count();
        if sectors < MIN_DISK_SECTORS {
            return Err(FsError::TooSmall);
        }
        let total = blocks_for(sectors);
        let (dj, di) = Geometry::defaults(total);
        let j = opts.journal_blocks.unwrap_or(dj);
        let ino = opts
            .inode_count
            .unwrap_or(di)
            .div_ceil(INODES_PER_BLOCK)
            .checked_mul(INODES_PER_BLOCK)
            .ok_or(FsError::TooSmall)?;
        let geo = Geometry::compute(total, j, ino).ok_or(FsError::TooSmall)?;
        let sb = Superblock {
            uuid: opts.uuid,
            created: opts.now,
            geo,
        };
        let mut cache = BlockCache::new(
            dev,
            FS_START_LBA,
            total as u64,
            opts.cache_blocks.unwrap_or(DEFAULT_CACHE_BLOCKS),
        );
        // 1. Whatever superblocks were there (primary, and the copy where ours
        //    would go) must not survive a half-finished format.
        let zero = [0u8; SECTOR_SIZE];
        cache.dev_mut().write_sectors(FS_START_LBA, &zero)?;
        let backup = FS_START_LBA + geo.backup_sb() as u64 * SECTORS_PER_BLOCK;
        cache.dev_mut().write_sectors(backup, &zero)?;
        cache.dev_mut().flush()?;
        // 2. Empty journal.
        let hdr = build_journal_header(0, &[], &[]);
        cache.write_direct(geo.jhdr as u64, &hdr, 0)?;
        // 3. Bitmaps with the fixed metadata marked used.
        let mut bbits = bits::new(total);
        for b in 0..geo.data_start {
            bits::set(&mut bbits, b);
        }
        bits::set(&mut bbits, geo.backup_sb());
        for i in 0..geo.bbitmap_blocks {
            let img = bitmap_image(&bbits, i);
            cache.write_direct((geo.bbitmap_start + i) as u64, &img, 0)?;
        }
        let mut ibits = bits::new(geo.inode_count);
        bits::set(&mut ibits, ROOT_INO - 1);
        bits::set(&mut ibits, TRASH_INO - 1);
        for i in 0..geo.ibitmap_blocks {
            let img = bitmap_image(&ibits, i);
            cache.write_direct((geo.ibitmap_start + i) as u64, &img, 0)?;
        }
        cache.flush()?;
        let mut fs = Self::open_with_cache(cache, sb, false)?;
        // 4. Root and /.trash.
        let now = opts.now;
        fs.txn(|fs| {
            let mut root = Inode::new(Kind::Dir, now, ROOT_INO);
            root.nlink = 1;
            let trash = Inode::new(Kind::Dir, now, ROOT_INO);
            fs.write_inode(ROOT_INO, &root)?;
            fs.write_inode(TRASH_INO, &trash)?;
            fs.dir_insert(ROOT_INO, TRASH_NAME, TRASH_INO, Kind::Dir, now)
        })?;
        fs.validate_roots()?;
        Ok(fs)
    }

    /// Make the filesystem visible: flush everything, then write the superblock
    /// (a single sector, so it is either wholly there or absent) and its copy.
    pub(crate) fn seal(&mut self) -> Result<(), FsError> {
        self.cache.flush()?;
        let enc = self.sb.encode();
        let backup = FS_START_LBA + self.geo.backup_sb() as u64 * SECTORS_PER_BLOCK;
        self.cache.dev_mut().write_sectors(FS_START_LBA, &enc)?;
        self.cache.dev_mut().flush()?;
        self.cache.dev_mut().write_sectors(backup, &enc)?;
        self.cache.dev_mut().flush()?;
        Ok(())
    }

    /// Mount with the default cache size. Replays the journal, validates the
    /// superblock, bitmaps, journal, root and trash. Never panics on a corrupt
    /// disk; returns [`FsError::BadSuperblock`] or [`FsError::Corrupt`].
    pub fn mount(dev: D) -> Result<Self, FsError> {
        Self::mount_with(dev, DEFAULT_CACHE_BLOCKS)
    }

    /// [`mount`](Self::mount) with a cache of `cache_blocks` 4 KiB blocks.
    pub fn mount_with(mut dev: D, cache_blocks: usize) -> Result<Self, FsError> {
        let sb = match read_sb_state(&mut dev)? {
            SbState::Valid(sb) => sb,
            _ => return Err(FsError::BadSuperblock),
        };
        let cache = BlockCache::new(dev, FS_START_LBA, sb.geo.total_blocks as u64, cache_blocks);
        let mut fs = Self::open_with_cache(cache, sb, true)?;
        fs.validate_roots()?;
        Ok(fs)
    }

    /// [`mount`](Self::mount) followed by a full [`fsck`](Self::fsck); fails
    /// with `Corrupt` if the check finds anything.
    pub fn mount_verified(dev: D) -> Result<Self, FsError> {
        let mut fs = Self::mount(dev)?;
        let rep = fs.fsck()?;
        if rep.is_clean() {
            Ok(fs)
        } else {
            Err(FsError::Corrupt("fsck found problems"))
        }
    }

    fn open_with_cache(
        mut cache: BlockCache<D>,
        sb: Superblock,
        _check: bool,
    ) -> Result<Self, FsError> {
        let geo = sb.geo;
        if cache.block_count() < geo.total_blocks as u64 {
            return Err(FsError::BadSuperblock);
        }
        let seq = Self::replay_journal(&mut cache, &geo)?;
        // Bitmaps, read once, straight from the device.
        let mut raw = alloc::vec![0u8; geo.bbitmap_blocks as usize * BLOCK_SIZE];
        cache.read_many(geo.bbitmap_start as u64, &mut raw)?;
        let mut bbits = bits::new(geo.total_blocks);
        for i in 0..geo.bbitmap_blocks {
            let o = i as usize * BLOCK_SIZE;
            load_bitmap_block(&raw[o..o + BLOCK_SIZE], i, &mut bbits)?;
        }
        bits::fix_padding(&mut bbits, geo.total_blocks);
        let mut raw = alloc::vec![0u8; geo.ibitmap_blocks as usize * BLOCK_SIZE];
        cache.read_many(geo.ibitmap_start as u64, &mut raw)?;
        let mut ibits = bits::new(geo.inode_count);
        for i in 0..geo.ibitmap_blocks {
            let o = i as usize * BLOCK_SIZE;
            load_bitmap_block(&raw[o..o + BLOCK_SIZE], i, &mut ibits)?;
        }
        bits::fix_padding(&mut ibits, geo.inode_count);
        // The fixed metadata must be marked used.
        if bits::next_free(&bbits, 0, geo.data_start).is_some()
            || !bits::get(&bbits, geo.backup_sb())
        {
            return Err(FsError::Corrupt("bitmap leaves the metadata uncovered"));
        }
        if !bits::get(&ibits, ROOT_INO - 1) || !bits::get(&ibits, TRASH_INO - 1) {
            return Err(FsError::Corrupt("root/trash inode not allocated"));
        }
        let free_blocks = bits::count_free(&bbits, geo.total_blocks);
        let free_inodes = bits::count_free(&ibits, geo.inode_count);
        Ok(Fs3 {
            cache,
            sb,
            geo,
            bbits,
            ibits,
            free_blocks,
            free_inodes,
            alloc_hint: geo.data_start,
            seq,
            tx: Tx::idle(),
            poisoned: false,
            commits: 0,
        })
    }

    /// Replay a committed-but-not-checkpointed transaction. Idempotent.
    /// Returns the journal sequence number.
    fn replay_journal(cache: &mut BlockCache<D>, geo: &Geometry) -> Result<u64, FsError> {
        let mut h = [0u8; BLOCK_SIZE];
        cache.read(geo.jhdr as u64, &mut h)?;
        let Some(parsed) = parse_journal_header(&h, geo.journal_blocks) else {
            return Ok(0);
        };
        let n = parsed.targets.len();
        if n == 0 {
            return Ok(parsed.seq);
        }
        let mut payload = alloc::vec![0u8; n * BLOCK_SIZE];
        cache.read_many(geo.jpayload as u64, &mut payload)?;
        if !journal_crc_ok(&h, &payload) {
            // A torn commit record: the transaction never committed.
            return Ok(parsed.seq);
        }
        // A target must be a metadata block that lives outside the journal and
        // the superblocks (a checksum-valid but hostile record must not be
        // able to overwrite them).
        for &t in &parsed.targets {
            if t < geo.bbitmap_start || t >= geo.backup_sb() {
                return Err(FsError::Corrupt("journal target out of range"));
            }
        }
        for (i, &t) in parsed.targets.iter().enumerate() {
            let mut blk = [0u8; BLOCK_SIZE];
            blk.copy_from_slice(&payload[i * BLOCK_SIZE..(i + 1) * BLOCK_SIZE]);
            cache.write_direct(t as u64, &blk, 0)?;
        }
        cache.flush()?;
        let clear = build_journal_header(parsed.seq, &[], &[]);
        cache.write_direct(geo.jhdr as u64, &clear, 0)?;
        cache.flush()?;
        Ok(parsed.seq)
    }

    fn validate_roots(&mut self) -> Result<(), FsError> {
        let root = self.read_inode(ROOT_INO)?;
        if root.kind != Kind::Dir || root.parent != ROOT_INO {
            return Err(FsError::Corrupt("bad root inode"));
        }
        let trash = self.read_inode(TRASH_INO)?;
        if trash.kind != Kind::Dir || trash.parent != ROOT_INO {
            return Err(FsError::Corrupt("bad trash inode"));
        }
        match self.dir_find(ROOT_INO, &root, TRASH_NAME)? {
            Some((TRASH_INO, Kind::Dir)) => Ok(()),
            _ => Err(FsError::Corrupt("root has no .trash entry")),
        }
    }

    /// Give back the device. Nothing is pending: every returned operation is
    /// already durable.
    pub fn into_device(self) -> D {
        self.cache.into_inner()
    }

    /// Borrow the device.
    pub fn device(&self) -> &D {
        self.cache.dev()
    }

    /// Mutably borrow the device (bypasses the cache; for tests).
    pub fn device_mut(&mut self) -> &mut D {
        self.cache.dev_mut()
    }

    /// Block-cache counters.
    pub fn cache_stats(&self) -> CacheStats {
        self.cache.stats()
    }

    /// Zero the block-cache counters.
    pub fn reset_cache_stats(&mut self) {
        self.cache.reset_stats();
    }

    /// Journal transactions committed since mount.
    pub fn commits(&self) -> u64 {
        self.commits
    }

    /// The superblock (UUID, creation time, geometry).
    pub fn superblock(&self) -> &Superblock {
        &self.sb
    }

    /// Capacity and free space.
    pub fn statfs(&self) -> StatFs {
        StatFs {
            block_size: BLOCK_SIZE as u32,
            total_blocks: self.geo.total_blocks,
            free_blocks: self.free_blocks,
            data_blocks: self.geo.total_blocks - self.geo.data_start - 1,
            total_inodes: self.geo.inode_count,
            free_inodes: self.free_inodes,
        }
    }

    /// Force a device barrier. Operations are already durable when they
    /// return; this exists for callers that want an explicit sync point.
    pub fn sync(&mut self) -> Result<(), FsError> {
        self.ready()?;
        self.cache.flush()?;
        Ok(())
    }

    pub(crate) fn ready(&self) -> Result<(), FsError> {
        if self.poisoned {
            Err(FsError::Poisoned)
        } else {
            Ok(())
        }
    }

    // -----------------------------------------------------------------------
    // Block access through the transaction overlay
    // -----------------------------------------------------------------------

    pub(crate) fn is_data_block(&self, b: u32) -> bool {
        b >= self.geo.data_start && b < self.geo.backup_sb()
    }

    /// Read a metadata block, seeing this transaction's uncommitted changes.
    pub(crate) fn meta_get(&mut self, blk: u32) -> Result<&Block, FsError> {
        if let Some(b) = self.tx.meta.get(&blk) {
            return Ok(b);
        }
        Ok(self.cache.get(blk as u64)?)
    }

    /// Get a metadata block for modification (copied into the overlay).
    pub(crate) fn meta_mut(&mut self, blk: u32) -> Result<&mut Block, FsError> {
        if !self.tx.meta.contains_key(&blk) {
            if self.tx.meta.len() >= self.geo.journal_blocks as usize {
                return Err(FsError::TxTooLarge);
            }
            let mut b = Box::new([0u8; BLOCK_SIZE]);
            self.cache.read(blk as u64, &mut b)?;
            self.tx.meta.insert(blk, b);
        }
        self.tx
            .meta
            .get_mut(&blk)
            .map(|b| &mut **b)
            .ok_or(FsError::Corrupt("transaction overlay"))
    }

    /// Replace a metadata block wholesale (no read needed).
    pub(crate) fn put_meta(&mut self, blk: u32, img: Block) -> Result<(), FsError> {
        if !self.tx.meta.contains_key(&blk)
            && self.tx.meta.len() >= self.geo.journal_blocks as usize
        {
            return Err(FsError::TxTooLarge);
        }
        self.tx.meta.insert(blk, Box::new(img));
        Ok(())
    }

    /// Read a directory (`ext = false`) or extent (`ext = true`) block,
    /// verifying its checksum once per load into the cache.
    pub(crate) fn typed_block(
        &mut self,
        blk: u32,
        ext: bool,
        owner: u32,
    ) -> Result<&Block, FsError> {
        if !self.is_data_block(blk) {
            return Err(FsError::Corrupt("metadata block out of range"));
        }
        if let Some(b) = self.tx.meta.get(&blk) {
            return Ok(b);
        }
        let want = 1 + ext as u8;
        let verified = {
            let (b, tag) = self.cache.get_tagged(blk as u64)?;
            if tag == want {
                false
            } else {
                verify_typed(b, ext, owner)?;
                true
            }
        };
        if verified {
            self.cache.set_tag(blk as u64, want);
        }
        Ok(self.cache.get(blk as u64)?)
    }

    /// Free a metadata block (directory / extent block) at commit, dropping any
    /// pending change to it.
    pub(crate) fn free_meta_block(&mut self, blk: u32) {
        self.tx.meta.remove(&blk);
        self.tx.to_free.push((blk, 1));
    }

    // -----------------------------------------------------------------------
    // Inodes
    // -----------------------------------------------------------------------

    fn inode_loc(&self, ino: Ino) -> Result<(u32, usize), FsError> {
        if ino == 0 || ino > self.geo.inode_count {
            return Err(FsError::NotFound);
        }
        let slot = ino - 1;
        Ok((
            self.geo.itable_start + slot / INODES_PER_BLOCK,
            (slot % INODES_PER_BLOCK) as usize * INODE_SIZE,
        ))
    }

    /// Read a live inode: `NotFound` if `ino` is out of range or unallocated.
    pub(crate) fn read_inode(&mut self, ino: Ino) -> Result<Inode, FsError> {
        let (blk, off) = self.inode_loc(ino)?;
        if !bits::get(&self.ibits, ino - 1) {
            return Err(FsError::NotFound);
        }
        let b = self.meta_get(blk)?;
        Inode::decode(&b[off..off + INODE_SIZE]).ok_or(FsError::Corrupt("inode checksum"))
    }

    pub(crate) fn write_inode(&mut self, ino: Ino, node: &Inode) -> Result<(), FsError> {
        let (blk, off) = self.inode_loc(ino)?;
        let b = self.meta_mut(blk)?;
        node.encode(&mut b[off..off + INODE_SIZE]);
        Ok(())
    }

    pub(crate) fn alloc_inode(&mut self) -> Result<Ino, FsError> {
        let bit = bits::next_free(&self.ibits, 0, self.geo.inode_count).ok_or(FsError::NoInodes)?;
        bits::set(&mut self.ibits, bit);
        self.tx.ilog.push((bit, true));
        self.tx.touched_ib.insert(bit / BITS_PER_BITMAP_BLOCK);
        self.free_inodes = self.free_inodes.saturating_sub(1);
        Ok(bit + 1)
    }

    pub(crate) fn free_inode(&mut self, ino: Ino) -> Result<(), FsError> {
        if ino == ROOT_INO || ino == TRASH_INO {
            return Err(FsError::Corrupt("freeing a fixed inode"));
        }
        let (blk, off) = self.inode_loc(ino)?;
        if !bits::get(&self.ibits, ino - 1) {
            return Err(FsError::Corrupt("double free of an inode"));
        }
        let b = self.meta_mut(blk)?;
        b[off..off + INODE_SIZE].fill(0);
        bits::clear(&mut self.ibits, ino - 1);
        self.tx.ilog.push((ino - 1, false));
        self.tx.touched_ib.insert((ino - 1) / BITS_PER_BITMAP_BLOCK);
        self.free_inodes += 1;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Block allocation (in the RAM bitmap; logged for rollback)
    // -----------------------------------------------------------------------

    /// Mark `[start, start+len)` used or free, checking the previous state.
    pub(crate) fn mark_range(&mut self, start: u32, len: u32, used: bool) -> Result<(), FsError> {
        if len == 0 {
            return Ok(());
        }
        let end = start
            .checked_add(len)
            .ok_or(FsError::Corrupt("block range overflow"))?;
        if start < self.geo.data_start || end > self.geo.backup_sb() {
            return Err(FsError::Corrupt("block range outside the data region"));
        }
        for b in start..end {
            if bits::get(&self.bbits, b) == used {
                return Err(FsError::Corrupt("block allocated or freed twice"));
            }
        }
        for b in start..end {
            if used {
                bits::set(&mut self.bbits, b);
            } else {
                bits::clear(&mut self.bbits, b);
            }
        }
        for i in start / BITS_PER_BITMAP_BLOCK..=(end - 1) / BITS_PER_BITMAP_BLOCK {
            self.tx.touched_bb.insert(i);
        }
        self.tx.blog.push((start, len, used));
        if used {
            self.free_blocks = self.free_blocks.saturating_sub(len);
        } else {
            self.free_blocks = self.free_blocks.saturating_add(len);
        }
        Ok(())
    }

    /// Allocate `want` blocks, preferring one contiguous run near `hint`;
    /// otherwise the largest available pieces. Returns `(start, len)` runs in
    /// allocation order.
    pub(crate) fn alloc_blocks(
        &mut self,
        want: u32,
        hint: u32,
    ) -> Result<Vec<(u32, u32)>, FsError> {
        if want == 0 {
            return Ok(Vec::new());
        }
        if want > self.free_blocks {
            return Err(FsError::NoSpace);
        }
        let lo = self.geo.data_start;
        let hi = self.geo.backup_sb();
        let hint = if hint < lo || hint >= hi { lo } else { hint };
        let start = bits::find_run(&self.bbits, hint, hi, want)
            .or_else(|| bits::find_run(&self.bbits, lo, hi, want));
        if let Some(s) = start {
            self.mark_range(s, want, true)?;
            self.alloc_hint = s + want;
            return Ok(alloc::vec![(s, want)]);
        }
        // Fragmented: take free runs from the hint onwards, then wrap around.
        let mut runs: Vec<(u32, u32)> = Vec::new();
        let mut remaining = want;
        for (from, to) in [(hint, hi), (lo, hint)] {
            let mut p = from;
            while remaining > 0 {
                let Some(s) = bits::next_free(&self.bbits, p, to) else {
                    break;
                };
                let e = bits::next_used(&self.bbits, s, to).unwrap_or(to);
                let take = (e - s).min(remaining);
                runs.push((s, take));
                remaining -= take;
                p = s + take;
            }
        }
        if remaining > 0 {
            return Err(FsError::NoSpace);
        }
        for &(s, l) in &runs {
            self.mark_range(s, l, true)?;
        }
        if let Some(&(s, l)) = runs.last() {
            self.alloc_hint = s + l;
        }
        Ok(runs)
    }

    // -----------------------------------------------------------------------
    // Transactions
    // -----------------------------------------------------------------------

    fn begin(&mut self) -> Result<(), FsError> {
        self.ready()?;
        self.tx = Tx::idle();
        self.tx.active = true;
        self.tx.free_blocks0 = self.free_blocks;
        self.tx.free_inodes0 = self.free_inodes;
        self.tx.hint0 = self.alloc_hint;
        Ok(())
    }

    /// Undo everything the transaction did in RAM (nothing reached the journal).
    fn abort(&mut self) {
        let tx = core::mem::replace(&mut self.tx, Tx::idle());
        for &(s, l, now) in tx.blog.iter().rev() {
            for b in s..s + l {
                if now {
                    bits::clear(&mut self.bbits, b);
                } else {
                    bits::set(&mut self.bbits, b);
                }
            }
        }
        for &(bit, now) in tx.ilog.iter().rev() {
            if now {
                bits::clear(&mut self.ibits, bit);
            } else {
                bits::set(&mut self.ibits, bit);
            }
        }
        self.free_blocks = tx.free_blocks0;
        self.free_inodes = tx.free_inodes0;
        self.alloc_hint = tx.hint0;
        // Data blocks written for the aborted transaction are garbage in free
        // blocks; make sure the cache never writes them back later.
        for &(s, l) in &tx.data {
            for b in s..s + l {
                self.cache.discard(b as u64);
            }
        }
    }

    /// Run `f` as one atomic, durable transaction.
    pub(crate) fn txn<R>(
        &mut self,
        f: impl FnOnce(&mut Self) -> Result<R, FsError>,
    ) -> Result<R, FsError> {
        self.begin()?;
        match f(self) {
            Ok(r) => {
                self.commit()?;
                Ok(r)
            }
            Err(e) => {
                self.abort();
                Err(e)
            }
        }
    }

    fn commit(&mut self) -> Result<(), FsError> {
        if let Err(e) = self.prepare_commit() {
            self.abort();
            return Err(e);
        }
        if self.tx.meta.is_empty() {
            self.tx = Tx::idle();
            return Ok(());
        }
        // From here on the transaction may be (partly) on the medium: any
        // failure leaves RAM and disk in an unknown relation, so poison.
        match self.write_journal() {
            Ok(()) => {
                self.commits += 1;
                self.tx = Tx::idle();
                Ok(())
            }
            Err(e) => {
                self.poisoned = true;
                self.tx = Tx::idle();
                Err(e)
            }
        }
    }

    /// Apply the deferred frees and turn the touched bitmap blocks into images.
    fn prepare_commit(&mut self) -> Result<(), FsError> {
        let frees = core::mem::take(&mut self.tx.to_free);
        for (s, l) in frees {
            self.mark_range(s, l, false)?;
        }
        let bb: Vec<u32> = self.tx.touched_bb.iter().copied().collect();
        for i in bb {
            let img = bitmap_image(&self.bbits, i);
            self.put_meta(self.geo.bbitmap_start + i, img)?;
        }
        let ib: Vec<u32> = self.tx.touched_ib.iter().copied().collect();
        for i in ib {
            let img = bitmap_image(&self.ibits, i);
            self.put_meta(self.geo.ibitmap_start + i, img)?;
        }
        if self.tx.meta.len() > self.geo.journal_blocks as usize {
            return Err(FsError::TxTooLarge);
        }
        Ok(())
    }

    fn write_journal(&mut self) -> Result<(), FsError> {
        let geo = self.geo;
        let targets: Vec<u32> = self.tx.meta.keys().copied().collect();
        let blocks: Vec<&Block> = self.tx.meta.values().map(|b| &**b).collect();
        // 1. Payload into the journal; the flush also makes the new data
        //    blocks (written copy-on-write by this transaction) durable.
        for (i, b) in blocks.iter().enumerate() {
            self.cache
                .write_direct((geo.jpayload + i as u32) as u64, b, 0)?;
        }
        self.cache.flush()?;
        // 2. Commit record. After this flush the transaction is committed.
        let seq = self.seq + 1;
        let hdr = build_journal_header(seq, &targets, &blocks);
        self.cache.write_direct(geo.jhdr as u64, &hdr, 0)?;
        self.cache.flush()?;
        // 3. Checkpoint to the home locations.
        for (t, b) in targets.iter().zip(blocks.iter()) {
            let tag = if b[4..8] == DIR_MAGIC {
                1
            } else if b[4..8] == EXT_MAGIC {
                2
            } else {
                0
            };
            self.cache.write_direct(*t as u64, b, tag)?;
        }
        self.cache.flush()?;
        // 4. Retire the record before the payload area is reused.
        let clear = build_journal_header(seq, &[], &[]);
        self.cache.write_direct(geo.jhdr as u64, &clear, 0)?;
        self.cache.flush()?;
        self.seq = seq;
        Ok(())
    }
}
