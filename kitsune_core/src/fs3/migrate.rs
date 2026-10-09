//! Migration of an OJFS v2 image (LBA 0, `OJF2`) to v3 (LBA 128).
//!
//! The v3 area is built completely — journal, bitmaps, every directory and
//! file, the trash — with **no superblock**, so the device still reads as v2
//! (`detect` → `V2`) the whole time. After a full `fsck` of the unsealed
//! result and a flush, the superblock goes down last, as a single sector. A
//! power cut at any point before that leaves the v2 image (sectors 0..127,
//! never written) untouched and the migration can simply be run again.

use super::inode::{FLAG_TRASHED, Kind};
use super::layout::{FS_START_LBA, MIN_DISK_SECTORS, ROOT_INO, TRASH_INO, TRASH_NAME};
use super::{FormatOptions, Fs3, FsError, Geometry, blocks_for};
use crate::blockdev::{BlockDevice, IoError, SECTOR_SIZE};
use crate::fs;
use alloc::collections::BTreeSet;
use alloc::vec::Vec;

/// Why a migration did not happen. In every case the v2 image is intact.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MigrateError {
    /// The image is not a formatted OJFS v2 image.
    NotV2,
    /// The device already holds a valid v3 filesystem; nothing is touched.
    AlreadyV3,
    /// The disk is smaller than 1 MiB, or cannot hold the v2 content.
    TooSmall,
    Io(IoError),
    /// The filesystem layer refused something (should not happen).
    Fs(FsError),
    /// The freshly built v3 failed its own `fsck`; it was not made visible.
    Verify,
}

impl From<IoError> for MigrateError {
    fn from(e: IoError) -> Self {
        MigrateError::Io(e)
    }
}

impl From<FsError> for MigrateError {
    fn from(e: FsError) -> Self {
        match e {
            FsError::TooSmall | FsError::NoSpace | FsError::NoInodes => MigrateError::TooSmall,
            FsError::Io(e) => MigrateError::Io(e),
            other => MigrateError::Fs(other),
        }
    }
}

/// What the migration did.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct MigrationReport {
    pub files: u32,
    pub dirs: u32,
    /// Items that ended up directly in `/.trash`.
    pub trashed: u32,
    /// Names changed because they were invalid in v3 or collided.
    pub renamed: u32,
    /// Records whose parent was invalid, trashed or in a cycle; moved to the root.
    pub orphans: u32,
    /// Records with an unknown state byte, skipped.
    pub skipped: u32,
    /// Payload bytes copied.
    pub bytes: u64,
}

/// Read the v2 image from the start of `dev` (99 sectors).
pub fn read_v2_image<D: BlockDevice>(dev: &mut D) -> Result<Vec<u8>, IoError> {
    let sectors = fs::IMAGE_SIZE.div_ceil(SECTOR_SIZE);
    let mut buf = alloc::vec![0u8; sectors * SECTOR_SIZE];
    dev.read_sectors(0, &mut buf)?;
    buf.truncate(fs::IMAGE_SIZE);
    Ok(buf)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Place {
    /// Live item in a live parent (`None` = the root).
    Active,
    /// Trashed item whose parent is live: becomes an entry of `/.trash`.
    TrashTop,
    /// Item inside another trashed or live parent: stays in the tree under it.
    Nested,
    /// Not a usable record.
    Skip,
}

struct Rec {
    place: Place,
    /// Parent slot for `Nested`, or the live parent for `TrashTop`/`Active`
    /// (`None` = root).
    parent: Option<usize>,
}

fn sanitize(raw: &[u8]) -> Vec<u8> {
    let mut v: Vec<u8> = raw
        .iter()
        .map(|&c| if c == b'/' || c == 0 { b'_' } else { c })
        .collect();
    if v.is_empty() {
        v.extend_from_slice(b"unnamed");
    }
    if v == b"." || v == b".." {
        v.insert(0, b'_');
    }
    v
}

/// `name`, or `name~2`, `name~3`, ... not in `used`; records the choice.
fn unique(used: &mut BTreeSet<Vec<u8>>, name: Vec<u8>) -> Vec<u8> {
    let mut cand = name.clone();
    let mut n = 2u32;
    while used.contains(&cand) {
        let suffix = alloc::format!("~{n}");
        cand = name[..name.len().min(super::MAX_NAME - suffix.len())].to_vec();
        cand.extend_from_slice(suffix.as_bytes());
        n += 1;
    }
    used.insert(cand.clone());
    cand
}

/// Convert the v2 `img` into a v3 filesystem on `dev` (from LBA 128), then make
/// it visible by writing the superblock last. Use `&mut dev` to keep the device.
///
/// * `TooSmall` (disk under 1 MiB, or content does not fit): nothing written.
/// * Any other error or a power cut before the final step: the device still
///   reads as v2 and the migration can be repeated.
///
/// Sectors `0..128` are never written.
pub fn migrate_v2<D: BlockDevice>(
    dev: &mut D,
    img: &[u8],
    opts: &FormatOptions,
) -> Result<MigrationReport, MigrateError> {
    if !fs::is_formatted(img) {
        return Err(MigrateError::NotV2);
    }
    // Never build over an existing v3 (even a damaged one): that would destroy
    // data the v2 image no longer reflects.
    match super::read_sb_state(dev)? {
        super::SbState::Valid(_) => return Err(MigrateError::AlreadyV3),
        super::SbState::Damaged => return Err(MigrateError::Fs(FsError::BadSuperblock)),
        super::SbState::Absent => {}
    }
    let sectors = dev.sector_count();
    if sectors < MIN_DISK_SECTORS {
        return Err(MigrateError::TooSmall);
    }

    // ---- plan: classify every record ----
    let n = fs::MAX_FILES;
    let mut recs: Vec<Rec> = Vec::with_capacity(n);
    let mut report = MigrationReport::default();
    for s in 0..n {
        let live = fs::is_active(img, s) || fs::is_trashed(img, s);
        if !live {
            if fs::is_used(img, s) {
                report.skipped += 1;
            }
            recs.push(Rec {
                place: Place::Skip,
                parent: None,
            });
            continue;
        }
        let trashed = fs::is_trashed(img, s);
        let p = fs::parent_at(img, s);
        let pslot = p as usize;
        let parent_ok = p != fs::ROOT
            && pslot < n
            && pslot != s
            && fs::is_dir(img, pslot)
            && (fs::is_active(img, pslot) || fs::is_trashed(img, pslot));
        let rec = if p == fs::ROOT {
            Rec {
                place: if trashed {
                    Place::TrashTop
                } else {
                    Place::Active
                },
                parent: None,
            }
        } else if !parent_ok {
            report.orphans += 1;
            Rec {
                place: if trashed {
                    Place::TrashTop
                } else {
                    Place::Active
                },
                parent: None,
            }
        } else {
            let parent_trashed = fs::is_trashed(img, pslot);
            match (trashed, parent_trashed) {
                (true, false) => Rec {
                    place: Place::TrashTop,
                    parent: Some(pslot),
                },
                (false, true) => {
                    report.orphans += 1;
                    Rec {
                        place: Place::Active,
                        parent: None,
                    }
                }
                _ => Rec {
                    place: Place::Nested,
                    parent: Some(pslot),
                },
            }
        };
        recs.push(rec);
    }
    // Break parent cycles among nested records: cut the first record found
    // whose ancestor chain does not reach the top.
    for s in 0..n {
        let mut cur = s;
        let mut steps = 0;
        while recs[cur].place == Place::Nested {
            match recs[cur].parent {
                Some(p) => cur = p,
                None => break,
            }
            steps += 1;
            if steps > n {
                report.orphans += 1;
                recs[s].place = if fs::is_trashed(img, s) {
                    Place::TrashTop
                } else {
                    Place::Active
                };
                recs[s].parent = None;
                break;
            }
        }
    }
    // Nested chains may still end in a Skip record only if the parent was
    // invalid, which `parent_ok` already excluded.

    // ---- capacity check before any write ----
    let total = blocks_for(sectors);
    let (dj, di) = Geometry::defaults(total);
    let geo = Geometry::compute(
        total,
        opts.journal_blocks.unwrap_or(dj),
        opts.inode_count.unwrap_or(di),
    )
    .ok_or(MigrateError::TooSmall)?;
    let mut need_blocks = 4u64; // root + trash directory blocks, slack
    let mut need_inodes = 2u64;
    for (s, r) in recs.iter().enumerate() {
        if r.place == Place::Skip {
            continue;
        }
        need_inodes += 1;
        need_blocks += if fs::is_dir(img, s) {
            1
        } else {
            fs::size_at(img, s).div_ceil(4096) as u64
        };
    }
    let free = total as u64 - geo.data_start as u64 - 1;
    if need_blocks > free || need_inodes > geo.inode_count as u64 {
        return Err(MigrateError::TooSmall);
    }

    // ---- build the v3 without a superblock ----
    let now = opts.now;
    let mut v3 = Fs3::format_unsealed(&mut *dev, opts)?;
    let mut used: Vec<BTreeSet<Vec<u8>>> = Vec::new(); // per created directory
    let mut dir_names: Vec<u32> = Vec::new(); // inode for each entry of `used`
    used.push([TRASH_NAME.to_vec()].into_iter().collect());
    dir_names.push(ROOT_INO);
    used.push(BTreeSet::new());
    dir_names.push(TRASH_INO);
    let used_idx = |dir_names: &[u32], ino: u32| dir_names.iter().position(|&d| d == ino);
    let mut inode_of: Vec<Option<u32>> = alloc::vec![None; n];

    // Creation order: parents before children. Repeat until nothing is left
    // (depth is bounded by the number of records).
    let mut pending: Vec<usize> = (0..n).filter(|&s| recs[s].place != Place::Skip).collect();
    while !pending.is_empty() {
        let before = pending.len();
        let mut next: Vec<usize> = Vec::new();
        for &s in &pending {
            let r = &recs[s];
            let is_dir = fs::is_dir(img, s);
            let kind = if is_dir { Kind::Dir } else { Kind::File };
            // Where does it go?
            let (parent_ino, trash_orig): (u32, Option<u32>) = match (r.place, r.parent) {
                (Place::Active, None) => (ROOT_INO, None),
                (Place::TrashTop, None) => (TRASH_INO, Some(ROOT_INO)),
                (Place::TrashTop, Some(p)) => match inode_of[p] {
                    Some(pi) => (TRASH_INO, Some(pi)),
                    None => {
                        next.push(s);
                        continue;
                    }
                },
                (Place::Nested, Some(p)) | (Place::Active, Some(p)) => match inode_of[p] {
                    Some(pi) => (pi, None),
                    None => {
                        next.push(s);
                        continue;
                    }
                },
                _ => continue,
            };
            let raw = sanitize(fs::name_at(img, s));
            let ui = match used_idx(&dir_names, parent_ino) {
                Some(i) => i,
                None => {
                    used.push(BTreeSet::new());
                    dir_names.push(parent_ino);
                    used.len() - 1
                }
            };
            let name = unique(&mut used[ui], raw.clone());
            if name != fs::name_at(img, s) {
                report.renamed += 1;
            }
            let data = if is_dir { None } else { fs::read_slot(img, s) };
            let ino = v3.txn(|f| {
                let ino = f.new_node(parent_ino, &name, kind, now)?;
                if let Some(d) = data {
                    f.write_inner(ino, 0, d, now)?;
                }
                if let Some(orig) = trash_orig {
                    let mut node = f.read_inode(ino)?;
                    node.flags |= FLAG_TRASHED;
                    node.trash_parent = orig;
                    node.trash_time = now;
                    // Every migrated directory (and the root) has ctime = now.
                    node.trash_pctime = now as u32;
                    node.trash_name = raw.clone();
                    f.write_inode(ino, &node)?;
                }
                Ok(ino)
            })?;
            inode_of[s] = Some(ino);
            if is_dir {
                report.dirs += 1;
            } else {
                report.files += 1;
                report.bytes += data.map_or(0, |d| d.len() as u64);
            }
            if trash_orig.is_some() {
                report.trashed += 1;
            }
        }
        if next.len() == before {
            // No progress: a record whose parent never got created. Cannot
            // happen after the planning above; refuse rather than loop.
            return Err(MigrateError::Verify);
        }
        pending = next;
    }

    // ---- verify, then make it visible (superblock last) ----
    let rep = v3.fsck()?;
    if !rep.is_clean() {
        return Err(MigrateError::Verify);
    }
    v3.seal()?;
    let _ = FS_START_LBA;
    Ok(report)
}
