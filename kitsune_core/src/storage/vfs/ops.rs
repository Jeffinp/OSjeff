//! ops (split out of `vfs.rs`).

use super::*;

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
        b"Bem-vindo ao Kitsune.\nGerenciador de arquivos:\n setas   navegam\n Del     manda pra lixeira\n Tab     alterna arquivos/lixeira\n Enter   abre\n",
    ),
    ("/notas.txt", b"Arquivo de exemplo do Kitsune."),
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
