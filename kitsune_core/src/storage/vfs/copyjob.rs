//! copyjob (split out of `vfs.rs`).

use super::*;

pub(super) enum Item {
    Dir {
        dst: Vec<u8>,
    },
    File {
        src: Vec<u8>,
        dst: Vec<u8>,
        size: u64,
    },
}

pub(super) struct Current {
    pub(super) src: Vec<u8>,
    pub(super) dst: Vec<u8>,
    pub(super) size: u64,
    pub(super) off: u64,
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
    pub(super) items: Vec<Item>,
    pub(super) next: usize,
    pub(super) cur: Option<Current>,
    pub(super) total_bytes: u64,
    pub(super) done_bytes: u64,
    pub(super) files_total: usize,
    pub(super) files_done: usize,
    pub(super) results: Vec<Vec<u8>>,
    pub(super) finished: bool,
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

    pub(super) fn check_room(&self) -> Result<()> {
        if self.items.len() >= MAX_COPY_ITEMS {
            Err(VfsError::TooBig)
        } else {
            Ok(())
        }
    }

    pub(super) fn push_file(&mut self, src: Vec<u8>, dst: Vec<u8>, size: u64) -> Result<()> {
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

    pub(super) fn step_inner<B: Backend + ?Sized>(
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
