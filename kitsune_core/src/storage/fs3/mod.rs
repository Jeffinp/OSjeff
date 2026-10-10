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

mod bitmap;
mod mount;
mod superblock;
use bitmap::*;
pub use superblock::*;

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

// ---------------------------------------------------------------------------
// Bitmap block images
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Construction: format, mount
// ---------------------------------------------------------------------------
