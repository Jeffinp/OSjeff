//! Desktop VFS: the file operations the file manager, the terminal and the
//! editor share, on top of an OJFS v3 volume.
//!
//! The kernel's `desktop/vfs.rs` is only glue (which disk, the RAM fallback, the
//! lock, the clock); every decision lives here so it runs on the host against a
//! [`RamDisk`](crate::blockdev::RamDisk):
//!
//! * [`Backend`]: the object-safe slice of [`Fs3`] the desktop needs (implemented
//!   for every `Fs3<D>`), so one code path serves the ATA disk and the RAM disk.
//! * [`VfsError`]: typed errors with a Portuguese [`message`](VfsError::message)
//!   ready to show (ASCII only: the bitmap font has no accents).
//! * Path and name helpers: [`join`], [`parent`], [`base_name`], [`validate_name`],
//!   [`unique_name`] (`"a (2).txt"`).
//! * Whole operations: [`move_to`] (rename, instant) and [`CopyJob`], a copy that
//!   runs in small steps so a multi-megabyte copy never freezes the compositor,
//!   can be cancelled, and cleans up the half-written file.
//! * [`seed_welcome`]: the three welcome files of a fresh disk (the only copy).
//!
//! Paths are absolute (`/a/b`), as `Fs3` wants them; `/` is the root and
//! `/.trash` is reserved (the trash lives there and is reached through
//! [`Backend::trash_list`] and friends).

use crate::blockdev::BlockDevice;
use crate::fs3::{self, DirEntry, Fs3, FsError, Kind, Stat, StatFs, TrashEntry};
use alloc::vec::Vec;

/// Longest name in bytes.
pub const MAX_NAME: usize = fs3::MAX_NAME;
/// Largest number of items one copy may plan.
pub const MAX_COPY_ITEMS: usize = 200_000;
/// Bytes copied per [`CopyJob::step`] by default.
pub const COPY_CHUNK: usize = 64 * 1024;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Why a desktop file operation failed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VfsError {
    NotFound,
    Exists,
    NotDir,
    IsDir,
    NotEmpty,
    /// Empty, `.`/`..`, `/`, NUL, control characters or not UTF-8.
    InvalidName,
    NameTooLong,
    InvalidPath,
    /// `/.trash` is managed by the system.
    Reserved,
    /// A folder cannot go inside itself.
    InvalidMove,
    NoSpace,
    NoInodes,
    TooBig,
    /// The filesystem is in use (lock not available).
    Busy,
    /// Nothing is mounted.
    Unavailable,
    /// The disk reported an error.
    Io,
    /// The on-disk structures are damaged.
    Corrupt,
    /// The user cancelled a long operation.
    Cancelled,
}

impl VfsError {
    /// A short Portuguese message (ASCII) for the status bar or a dialog.
    pub fn message(self) -> &'static str {
        match self {
            VfsError::NotFound => "Item nao encontrado",
            VfsError::Exists => "Ja existe um item com esse nome",
            VfsError::NotDir => "O destino nao e uma pasta",
            VfsError::IsDir => "O item e uma pasta",
            VfsError::NotEmpty => "A pasta nao esta vazia",
            VfsError::InvalidName => "Nome invalido",
            VfsError::NameTooLong => "Nome longo demais (maximo 255 bytes)",
            VfsError::InvalidPath => "Caminho invalido",
            VfsError::Reserved => "Item reservado do sistema",
            VfsError::InvalidMove => "Nao e possivel mover uma pasta para dentro dela mesma",
            VfsError::NoSpace => "Disco cheio",
            VfsError::NoInodes => "Limite de arquivos do disco atingido",
            VfsError::TooBig => "Arquivo grande demais",
            VfsError::Busy => "Sistema de arquivos ocupado",
            VfsError::Unavailable => "Sem sistema de arquivos",
            VfsError::Io => "Erro de leitura/escrita no disco",
            VfsError::Corrupt => "Sistema de arquivos danificado",
            VfsError::Cancelled => "Operacao cancelada",
        }
    }
}

impl From<FsError> for VfsError {
    fn from(e: FsError) -> Self {
        match e {
            FsError::NotFound => VfsError::NotFound,
            FsError::Exists => VfsError::Exists,
            FsError::NotDir => VfsError::NotDir,
            FsError::IsDir => VfsError::IsDir,
            FsError::NotEmpty => VfsError::NotEmpty,
            FsError::InvalidName => VfsError::InvalidName,
            FsError::NameTooLong => VfsError::NameTooLong,
            FsError::InvalidPath => VfsError::InvalidPath,
            FsError::Reserved => VfsError::Reserved,
            FsError::InvalidMove => VfsError::InvalidMove,
            FsError::NoSpace => VfsError::NoSpace,
            FsError::NoInodes => VfsError::NoInodes,
            FsError::TooBig => VfsError::TooBig,
            FsError::TxTooLarge => VfsError::NoSpace,
            FsError::TooSmall => VfsError::NoSpace,
            FsError::BadSuperblock | FsError::Corrupt(_) => VfsError::Corrupt,
            FsError::Poisoned | FsError::Io(_) => VfsError::Io,
        }
    }
}

/// Result alias of this module.
pub type Result<T> = core::result::Result<T, VfsError>;

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// File or folder.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EntryKind {
    File,
    Dir,
}

impl From<Kind> for EntryKind {
    fn from(k: Kind) -> Self {
        match k {
            Kind::File => EntryKind::File,
            Kind::Dir => EntryKind::Dir,
        }
    }
}

/// One item of a folder listing.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Entry {
    pub name: Vec<u8>,
    pub kind: EntryKind,
    pub size: u64,
    /// Seconds since the Unix epoch (UTC).
    pub mtime: u64,
}

impl From<DirEntry> for Entry {
    fn from(e: DirEntry) -> Self {
        Entry {
            name: e.name,
            kind: e.kind.into(),
            size: e.size,
            mtime: e.mtime,
        }
    }
}

/// `stat` of one path.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Info {
    pub kind: EntryKind,
    pub size: u64,
    pub ctime: u64,
    pub mtime: u64,
    /// Allocated 4 KiB blocks.
    pub blocks: u32,
}

impl From<Stat> for Info {
    fn from(s: Stat) -> Self {
        Info {
            kind: s.kind.into(),
            size: s.size,
            ctime: s.ctime,
            mtime: s.mtime,
            blocks: s.blocks,
        }
    }
}

/// One item in the trash.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TrashItem {
    /// The key for [`Backend::trash_restore`] / [`Backend::trash_purge`].
    pub id: Vec<u8>,
    /// The name it had before it was deleted.
    pub name: Vec<u8>,
    pub kind: EntryKind,
    pub size: u64,
    pub deleted_at: u64,
}

impl From<TrashEntry> for TrashItem {
    fn from(t: TrashEntry) -> Self {
        TrashItem {
            id: t.trash_name,
            name: t.orig_name,
            kind: t.kind.into(),
            size: t.size,
            deleted_at: t.deleted_at,
        }
    }
}

/// Disk usage.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Usage {
    pub total: u64,
    pub free: u64,
}

impl Usage {
    /// Used bytes.
    pub fn used(&self) -> u64 {
        self.total.saturating_sub(self.free)
    }

    /// Used share in permille (0..=1000).
    pub fn used_permille(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        (self.used() as u128 * 1000 / self.total as u128).min(1000) as u32
    }
}

impl From<StatFs> for Usage {
    fn from(s: StatFs) -> Self {
        Usage {
            total: s.data_bytes(),
            free: s.free_bytes(),
        }
    }
}

// ---------------------------------------------------------------------------
// Backend
// ---------------------------------------------------------------------------

/// The filesystem operations the desktop uses, object-safe so the kernel can hand
/// a `&mut dyn Backend` of either the ATA volume or the RAM volume to the same code.
/// Every `Ok` of a mutating call is already durable on a real disk.
pub trait Backend {
    fn stat(&mut self, path: &[u8]) -> Result<Info>;
    fn readdir(&mut self, path: &[u8]) -> Result<Vec<Entry>>;
    fn read_file(&mut self, path: &[u8]) -> Result<Vec<u8>>;
    fn read_at(&mut self, path: &[u8], off: u64, buf: &mut [u8]) -> Result<usize>;
    /// Create or replace a whole file, atomically.
    fn write_file(&mut self, path: &[u8], data: &[u8], now: u64) -> Result<()>;
    fn create(&mut self, path: &[u8], now: u64) -> Result<()>;
    fn write_at(&mut self, path: &[u8], off: u64, data: &[u8], now: u64) -> Result<()>;
    fn append(&mut self, path: &[u8], data: &[u8], now: u64) -> Result<()>;
    fn mkdir(&mut self, path: &[u8], now: u64) -> Result<()>;
    /// Rename or move; an existing destination is an error.
    fn rename(&mut self, from: &[u8], to: &[u8], now: u64) -> Result<()>;
    /// Permanent delete (file or whole tree).
    fn remove_all(&mut self, path: &[u8]) -> Result<()>;
    /// Move to the trash.
    fn trash(&mut self, path: &[u8], now: u64) -> Result<()>;
    fn trash_list(&mut self) -> Result<Vec<TrashItem>>;
    /// Restore; returns the path it came back to.
    fn trash_restore(&mut self, id: &[u8], now: u64) -> Result<Vec<u8>>;
    fn trash_purge(&mut self, id: &[u8]) -> Result<()>;
    fn empty_trash(&mut self) -> Result<()>;
    fn usage(&mut self) -> Usage;
}

impl<D: BlockDevice> Backend for Fs3<D> {
    fn stat(&mut self, path: &[u8]) -> Result<Info> {
        Ok(Fs3::stat(self, path)?.into())
    }
    fn readdir(&mut self, path: &[u8]) -> Result<Vec<Entry>> {
        Ok(Fs3::readdir(self, path)?
            .into_iter()
            .map(Entry::from)
            .collect())
    }
    fn read_file(&mut self, path: &[u8]) -> Result<Vec<u8>> {
        Ok(Fs3::read_file(self, path)?)
    }
    fn read_at(&mut self, path: &[u8], off: u64, buf: &mut [u8]) -> Result<usize> {
        let ino = Fs3::open(self, path)?;
        Ok(Fs3::read_at(self, ino, off, buf)?)
    }
    fn write_file(&mut self, path: &[u8], data: &[u8], now: u64) -> Result<()> {
        Ok(Fs3::write_file(self, path, data, now)?)
    }
    fn create(&mut self, path: &[u8], now: u64) -> Result<()> {
        Fs3::create(self, path, now)?;
        Ok(())
    }
    fn write_at(&mut self, path: &[u8], off: u64, data: &[u8], now: u64) -> Result<()> {
        let ino = Fs3::open(self, path)?;
        Ok(Fs3::write_at(self, ino, off, data, now)?)
    }
    fn append(&mut self, path: &[u8], data: &[u8], now: u64) -> Result<()> {
        let ino = Fs3::open(self, path)?;
        Ok(Fs3::append(self, ino, data, now)?)
    }
    fn mkdir(&mut self, path: &[u8], now: u64) -> Result<()> {
        Fs3::mkdir(self, path, now)?;
        Ok(())
    }
    fn rename(&mut self, from: &[u8], to: &[u8], now: u64) -> Result<()> {
        Ok(Fs3::rename(self, from, to, now)?)
    }
    fn remove_all(&mut self, path: &[u8]) -> Result<()> {
        Ok(Fs3::remove_all(self, path)?)
    }
    fn trash(&mut self, path: &[u8], now: u64) -> Result<()> {
        Ok(Fs3::trash(self, path, now)?)
    }
    fn trash_list(&mut self) -> Result<Vec<TrashItem>> {
        Ok(Fs3::trash_list(self)?
            .into_iter()
            .map(TrashItem::from)
            .collect())
    }
    fn trash_restore(&mut self, id: &[u8], now: u64) -> Result<Vec<u8>> {
        Ok(Fs3::trash_restore(self, id, now)?)
    }
    fn trash_purge(&mut self, id: &[u8]) -> Result<()> {
        Ok(Fs3::trash_purge(self, id)?)
    }
    fn empty_trash(&mut self) -> Result<()> {
        Ok(Fs3::empty_trash(self)?)
    }
    fn usage(&mut self) -> Usage {
        Fs3::statfs(self).into()
    }
}

// ---------------------------------------------------------------------------
// Paths and names
// ---------------------------------------------------------------------------

/// `dir` + `/` + `name` (`dir == "/"` gives `/name`).
pub fn join(dir: &[u8], name: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(dir.len() + 1 + name.len());
    out.extend_from_slice(dir);
    if dir != b"/" {
        out.push(b'/');
    }
    out.extend_from_slice(name);
    out
}

/// The folder containing `path` (`/` for a top-level item and for `/` itself).
pub fn parent(path: &[u8]) -> Vec<u8> {
    match path.iter().rposition(|&b| b == b'/') {
        Some(0) | None => b"/".to_vec(),
        Some(i) => path[..i].to_vec(),
    }
}

/// The last component of `path` (empty for `/`).
pub fn base_name(path: &[u8]) -> &[u8] {
    match path.iter().rposition(|&b| b == b'/') {
        Some(i) => &path[i + 1..],
        None => path,
    }
}

/// True for `/`.
pub fn is_root(path: &[u8]) -> bool {
    path == b"/"
}

/// True if `path` is `ancestor` or lies below it (component-wise).
pub fn is_inside(path: &[u8], ancestor: &[u8]) -> bool {
    if is_root(ancestor) {
        return path.first() == Some(&b'/');
    }
    path == ancestor || (path.starts_with(ancestor) && path.get(ancestor.len()) == Some(&b'/'))
}

/// The components of an absolute path (`/a/b` gives `["a", "b"]`).
pub fn components(path: &[u8]) -> Vec<&[u8]> {
    path.split(|&b| b == b'/')
        .filter(|c| !c.is_empty())
        .collect()
}

/// Strip surrounding ASCII spaces from a typed name.
pub fn trim_name(raw: &[u8]) -> &[u8] {
    let mut s = raw;
    while let [b' ', rest @ ..] = s {
        s = rest;
    }
    while let [rest @ .., b' '] = s {
        s = rest;
    }
    s
}

/// Check a file or folder name: 1..=255 bytes of valid UTF-8, no `/`, NUL or control
/// characters, not `.` or `..`.
pub fn validate_name(name: &[u8]) -> Result<()> {
    if name.is_empty() || name == b"." || name == b".." {
        return Err(VfsError::InvalidName);
    }
    if name.len() > MAX_NAME {
        return Err(VfsError::NameTooLong);
    }
    if name.iter().any(|&b| b == b'/' || b < 0x20 || b == 0x7F)
        || core::str::from_utf8(name).is_err()
    {
        return Err(VfsError::InvalidName);
    }
    Ok(())
}

/// Split `name` into stem and extension (with the dot). A leading dot (`.profile`)
/// or a trailing one does not start an extension.
pub fn split_ext(name: &[u8]) -> (&[u8], &[u8]) {
    match name.iter().rposition(|&b| b == b'.') {
        Some(i) if i > 0 && i + 1 < name.len() => (&name[..i], &name[i..]),
        _ => (name, &[]),
    }
}

/// Remove a trailing `" (N)"` (N decimal) from a stem; returns the bare stem.
fn strip_copy_suffix(stem: &[u8]) -> &[u8] {
    let Some((b')', body)) = stem.split_last() else {
        return stem;
    };
    let Some(open) = body.iter().rposition(|&b| b == b'(') else {
        return stem;
    };
    let digits = &body[open + 1..];
    if open >= 1
        && body[open - 1] == b' '
        && !digits.is_empty()
        && digits.iter().all(u8::is_ascii_digit)
    {
        return &body[..open - 1];
    }
    stem
}

/// A name that is free according to `taken`: `name` itself if it is, else
/// `stem (2).ext`, `stem (3).ext`, ... An existing `" (N)"` in the stem is
/// replaced, so copying `a (2).txt` yields `a (3).txt`, not `a (2) (2).txt`.
/// The result never exceeds 255 bytes (the stem is cut on a UTF-8 boundary).
pub fn unique_name(name: &[u8], mut taken: impl FnMut(&[u8]) -> bool) -> Vec<u8> {
    if !taken(name) {
        return name.to_vec();
    }
    let (stem, ext) = split_ext(name);
    let stem = strip_copy_suffix(stem);
    for n in 2u32..100_000 {
        let mut tail = Vec::new();
        tail.extend_from_slice(b" (");
        push_u32(&mut tail, n);
        tail.push(b')');
        tail.extend_from_slice(ext);
        let room = MAX_NAME.saturating_sub(tail.len());
        let mut cut = stem.len().min(room);
        // Back up to a UTF-8 character boundary (continuation bytes are 10xxxxxx).
        while cut > 0 && cut < stem.len() && stem[cut] & 0xC0 == 0x80 {
            cut -= 1;
        }
        let mut cand = stem[..cut].to_vec();
        cand.extend_from_slice(&tail);
        if !taken(&cand) {
            return cand;
        }
    }
    name.to_vec()
}

fn push_u32(out: &mut Vec<u8>, mut v: u32) {
    let mut tmp = [0u8; 10];
    let mut i = tmp.len();
    loop {
        i -= 1;
        tmp[i] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    out.extend_from_slice(&tmp[i..]);
}

// ---------------------------------------------------------------------------
// Whole operations
// ---------------------------------------------------------------------------

/// True if `path` exists.
pub fn exists<B: Backend + ?Sized>(b: &mut B, path: &[u8]) -> bool {
    b.stat(path).is_ok()
}

/// List a folder (storage order; sort with `fileman::sort_entries`).
pub fn list<B: Backend + ?Sized>(b: &mut B, dir: &[u8]) -> Result<Vec<Entry>> {
    b.readdir(dir)
}

/// Create an empty file `dir/name`.
pub fn new_file<B: Backend + ?Sized>(
    b: &mut B,
    dir: &[u8],
    name: &[u8],
    now: u64,
) -> Result<Vec<u8>> {
    let name = trim_name(name);
    validate_name(name)?;
    let path = join(dir, name);
    b.create(&path, now)?;
    Ok(path)
}

/// Create a folder `dir/name`.
pub fn new_folder<B: Backend + ?Sized>(
    b: &mut B,
    dir: &[u8],
    name: &[u8],
    now: u64,
) -> Result<Vec<u8>> {
    let name = trim_name(name);
    validate_name(name)?;
    let path = join(dir, name);
    b.mkdir(&path, now)?;
    Ok(path)
}

/// Rename `path` to `new_name` in the same folder; returns the new path. Renaming to
/// the same name is a no-op.
pub fn rename_in<B: Backend + ?Sized>(
    b: &mut B,
    path: &[u8],
    new_name: &[u8],
    now: u64,
) -> Result<Vec<u8>> {
    let new_name = trim_name(new_name);
    validate_name(new_name)?;
    if base_name(path) == new_name {
        return Ok(path.to_vec());
    }
    let to = join(&parent(path), new_name);
    b.rename(path, &to, now)?;
    Ok(to)
}

/// What [`move_to`] did.
#[derive(Debug, PartialEq, Eq, Default)]
pub struct MoveReport {
    /// New paths of the items moved (or already in place).
    pub moved: Vec<Vec<u8>>,
    /// The first failure; the items after it were not touched.
    pub error: Option<VfsError>,
}

/// Move `sources` into the folder `dest`. A name already taken there gets a
/// `(2)` suffix; an item already in `dest` stays; a folder into itself or its own
/// subtree fails with [`VfsError::InvalidMove`]. A rename is O(1): instant for any size.
pub fn move_to<B: Backend + ?Sized>(
    b: &mut B,
    sources: &[Vec<u8>],
    dest: &[u8],
    now: u64,
) -> MoveReport {
    let mut rep = MoveReport::default();
    match b.stat(dest) {
        Ok(i) if i.kind == EntryKind::Dir => {}
        Ok(_) => {
            rep.error = Some(VfsError::NotDir);
            return rep;
        }
        Err(e) => {
            rep.error = Some(e);
            return rep;
        }
    }
    for src in sources {
        if parent(src) == dest {
            rep.moved.push(src.clone());
            continue;
        }
        if is_inside(dest, src) {
            rep.error = Some(VfsError::InvalidMove);
            return rep;
        }
        let name = unique_name(base_name(src), |n| exists(b, &join(dest, n)));
        let to = join(dest, &name);
        match b.rename(src, &to, now) {
            Ok(()) => rep.moved.push(to),
            Err(e) => {
                rep.error = Some(e);
                return rep;
            }
        }
    }
    rep
}

/// Items and bytes below `path` (a file counts itself).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct TreeSize {
    pub files: u64,
    pub dirs: u64,
    pub bytes: u64,
}

/// Walk `path` and total it up (explicit stack: depth is not limited by the call stack).
pub fn tree_size<B: Backend + ?Sized>(b: &mut B, path: &[u8]) -> Result<TreeSize> {
    let mut t = TreeSize::default();
    let info = b.stat(path)?;
    if info.kind == EntryKind::File {
        t.files = 1;
        t.bytes = info.size;
        return Ok(t);
    }
    let mut stack = alloc::vec![path.to_vec()];
    let mut seen = 0usize;
    while let Some(dir) = stack.pop() {
        t.dirs += 1;
        for e in b.readdir(&dir)? {
            seen += 1;
            if seen > MAX_COPY_ITEMS * 4 {
                return Err(VfsError::TooBig);
            }
            match e.kind {
                EntryKind::File => {
                    t.files += 1;
                    t.bytes += e.size;
                }
                EntryKind::Dir => stack.push(join(&dir, &e.name)),
            }
        }
    }
    Ok(t)
}

/// Move `path` to the trash.
pub fn remove<B: Backend + ?Sized>(b: &mut B, path: &[u8], now: u64) -> Result<()> {
    if is_root(path) {
        return Err(VfsError::Reserved);
    }
    b.trash(path, now)
}

/// Delete `path` for good (no trash).
pub fn purge<B: Backend + ?Sized>(b: &mut B, path: &[u8]) -> Result<()> {
    if is_root(path) {
        return Err(VfsError::Reserved);
    }
    b.remove_all(path)
}

/// The text of the three welcome files of a fresh disk, as `(path, content)`.
pub const WELCOME_FILES: [(&str, &[u8]); 3] = [
    (
        "/leiame.txt",
        b"Bem-vindo ao OSjeff.\nGerenciador de arquivos:\n setas   navegam\n Del     manda pra lixeira\n Tab     alterna arquivos/lixeira\n Enter   abre\n",
    ),
    ("/notas.txt", b"Arquivo de exemplo do OSjeff."),
    (
        "/Documentos/projeto.txt",
        b"Arquivo dentro de uma pasta.",
    ),
];

/// Write the welcome files (and the `Documentos` folder) into a fresh volume. This is
/// the only copy of the seed: the storage service and the RAM fallback both call it.
pub fn seed_welcome<B: Backend + ?Sized>(b: &mut B, now: u64) -> Result<()> {
    b.write_file(WELCOME_FILES[0].0.as_bytes(), WELCOME_FILES[0].1, now)?;
    b.write_file(WELCOME_FILES[1].0.as_bytes(), WELCOME_FILES[1].1, now)?;
    b.mkdir(b"/Documentos", now)?;
    b.write_file(WELCOME_FILES[2].0.as_bytes(), WELCOME_FILES[2].1, now)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Copy job
// ---------------------------------------------------------------------------

enum Item {
    Dir {
        dst: Vec<u8>,
    },
    File {
        src: Vec<u8>,
        dst: Vec<u8>,
        size: u64,
    },
}

struct Current {
    src: Vec<u8>,
    dst: Vec<u8>,
    size: u64,
    off: u64,
}

/// Where a [`CopyJob`] stands after a step.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Progress {
    Running,
    Done,
}

/// A recursive copy done in bounded steps. [`plan`](CopyJob::plan) walks the
/// sources and picks collision-free destination names; each
/// [`step`](CopyJob::step) then copies at most `budget` bytes (creating folders and
/// empty files costs nothing), so the caller can draw a progress bar between steps
/// and honour a cancel. A failed or aborted job deletes the file it was writing;
/// what was already copied stays (the volume is consistent at every step).
pub struct CopyJob {
    items: Vec<Item>,
    next: usize,
    cur: Option<Current>,
    total_bytes: u64,
    done_bytes: u64,
    files_total: usize,
    files_done: usize,
    results: Vec<Vec<u8>>,
    finished: bool,
}

impl CopyJob {
    /// Plan copying every path of `sources` into the folder `dest`. A source whose
    /// name is taken in `dest` (or by an earlier source) gets a `(2)` suffix, so
    /// copying into the folder it already lives in duplicates it. A folder
    /// cannot be copied into itself or its own subtree ([`VfsError::InvalidMove`]).
    pub fn plan<B: Backend + ?Sized>(b: &mut B, sources: &[Vec<u8>], dest: &[u8]) -> Result<Self> {
        match b.stat(dest)? {
            i if i.kind == EntryKind::Dir => {}
            _ => return Err(VfsError::NotDir),
        }
        let mut job = CopyJob {
            items: Vec::new(),
            next: 0,
            cur: None,
            total_bytes: 0,
            done_bytes: 0,
            files_total: 0,
            files_done: 0,
            results: Vec::new(),
            finished: false,
        };
        let mut taken: Vec<Vec<u8>> = Vec::new();
        for src in sources {
            let info = b.stat(src)?;
            if info.kind == EntryKind::Dir && is_inside(dest, src) {
                return Err(VfsError::InvalidMove);
            }
            let name = unique_name(base_name(src), |n| {
                taken.iter().any(|t| t == n) || exists(b, &join(dest, n))
            });
            taken.push(name.clone());
            let dst = join(dest, &name);
            job.results.push(dst.clone());
            match info.kind {
                EntryKind::File => job.push_file(src.clone(), dst, info.size)?,
                EntryKind::Dir => {
                    job.items.push(Item::Dir { dst: dst.clone() });
                    // Depth-first with an explicit stack; a folder's own item is
                    // already queued, so children always follow their parent.
                    let mut stack = alloc::vec![(src.clone(), dst)];
                    while let Some((sdir, ddir)) = stack.pop() {
                        for e in b.readdir(&sdir)? {
                            let (s, d) = (join(&sdir, &e.name), join(&ddir, &e.name));
                            match e.kind {
                                EntryKind::File => job.push_file(s, d, e.size)?,
                                EntryKind::Dir => {
                                    job.check_room()?;
                                    job.items.push(Item::Dir { dst: d.clone() });
                                    stack.push((s, d));
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(job)
    }

    fn check_room(&self) -> Result<()> {
        if self.items.len() >= MAX_COPY_ITEMS {
            Err(VfsError::TooBig)
        } else {
            Ok(())
        }
    }

    fn push_file(&mut self, src: Vec<u8>, dst: Vec<u8>, size: u64) -> Result<()> {
        self.check_room()?;
        self.total_bytes += size;
        self.files_total += 1;
        self.items.push(Item::File { src, dst, size });
        Ok(())
    }

    /// Bytes to copy in total.
    pub fn total_bytes(&self) -> u64 {
        self.total_bytes
    }

    /// Bytes copied so far.
    pub fn done_bytes(&self) -> u64 {
        self.done_bytes
    }

    /// Files to copy / already copied.
    pub fn files(&self) -> (usize, usize) {
        (self.files_done, self.files_total)
    }

    /// Where the top-level sources go (one path per source, in order).
    pub fn results(&self) -> &[Vec<u8>] {
        &self.results
    }

    /// Progress in permille (a job with no bytes reports by items).
    pub fn permille(&self) -> u32 {
        if self.finished {
            return 1000;
        }
        if self.total_bytes > 0 {
            return (self.done_bytes as u128 * 1000 / self.total_bytes as u128).min(1000) as u32;
        }
        if self.items.is_empty() {
            return 1000;
        }
        (self.next as u64 * 1000 / self.items.len() as u64) as u32
    }

    /// Name of the file being copied (for the status line).
    pub fn current_name(&self) -> &[u8] {
        match (&self.cur, self.items.get(self.next)) {
            (Some(c), _) => base_name(&c.dst),
            (None, Some(Item::File { dst, .. })) | (None, Some(Item::Dir { dst })) => {
                base_name(dst)
            }
            _ => &[],
        }
    }

    /// Do up to `budget` bytes of copying. On error the partial file is removed and the
    /// job is over (call nothing else on it).
    pub fn step<B: Backend + ?Sized>(
        &mut self,
        b: &mut B,
        budget: usize,
        now: u64,
    ) -> Result<Progress> {
        if self.finished {
            return Ok(Progress::Done);
        }
        let r = self.step_inner(b, budget.max(1), now);
        if r.is_err() {
            self.abort(b);
        }
        r
    }

    fn step_inner<B: Backend + ?Sized>(
        &mut self,
        b: &mut B,
        budget: usize,
        now: u64,
    ) -> Result<Progress> {
        let mut left = budget as u64;
        loop {
            if self.cur.is_none() {
                let Some(item) = self.items.get(self.next) else {
                    self.finished = true;
                    return Ok(Progress::Done);
                };
                match item {
                    Item::Dir { dst } => {
                        b.mkdir(dst, now)?;
                        self.next += 1;
                        continue;
                    }
                    Item::File { src, dst, size } => {
                        b.create(dst, now)?;
                        self.cur = Some(Current {
                            src: src.clone(),
                            dst: dst.clone(),
                            size: *size,
                            off: 0,
                        });
                    }
                }
            }
            let Some(cur) = self.cur.as_mut() else {
                continue;
            };
            let want = (cur.size - cur.off).min(left) as usize;
            if want > 0 {
                let mut buf = alloc::vec![0u8; want];
                let n = b.read_at(&cur.src, cur.off, &mut buf)?;
                if n == 0 {
                    // The source shrank while copying: stop at what it has.
                    cur.size = cur.off;
                } else {
                    b.write_at(&cur.dst, cur.off, &buf[..n], now)?;
                    cur.off += n as u64;
                    self.done_bytes += n as u64;
                    left = left.saturating_sub(n as u64);
                }
            }
            if cur.off >= cur.size {
                self.cur = None;
                self.next += 1;
                self.files_done += 1;
            }
            if left == 0 {
                return Ok(if self.next >= self.items.len() && self.cur.is_none() {
                    self.finished = true;
                    Progress::Done
                } else {
                    Progress::Running
                });
            }
        }
    }

    /// Stop now (cancel or failure): delete the half-written file. Completed copies stay.
    pub fn abort<B: Backend + ?Sized>(&mut self, b: &mut B) {
        if let Some(c) = self.cur.take() {
            let _ = b.remove_all(&c.dst);
        }
        self.finished = true;
    }
}

#[cfg(test)]
mod tests;
