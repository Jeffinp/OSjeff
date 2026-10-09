//! `MemFs`: an in-memory [`AppFs`] (tests, and the kernel's stand-in while
//! `kernel::storage` is not wired).

use super::path::{self, within};
use super::{AppFs, DirEntry, ENTRY_OVERHEAD, FsError, Kind, Stat};
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Largest single file `MemFs` will hold.
pub const MAX_FILE: u64 = 16 << 20;
/// Most entries `MemFs` will hold.
pub const MAX_ENTRIES: usize = 4096;

enum Node {
    Dir,
    File(Vec<u8>),
}

/// Flat map from canonical path to node; `"/"` always exists.
pub struct MemFs {
    nodes: BTreeMap<String, Node>,
    /// Cap on the sum of file bytes.
    max_bytes: u64,
    bytes: u64,
}

impl MemFs {
    /// An empty filesystem holding at most `max_bytes` of file data.
    pub fn new(max_bytes: u64) -> MemFs {
        let mut nodes = BTreeMap::new();
        nodes.insert(String::from("/"), Node::Dir);
        MemFs {
            nodes,
            max_bytes,
            bytes: 0,
        }
    }

    /// Total file bytes stored.
    pub fn used_bytes(&self) -> u64 {
        self.bytes
    }

    /// Number of entries (including the root).
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.len() <= 1
    }

    /// Defensive re-validation: backends never trust their caller.
    fn check<'a>(&self, p: &'a str) -> Result<&'a str, FsError> {
        match path::normalize(p.as_bytes()) {
            Ok(n) if n == p => Ok(p),
            _ => Err(FsError::Invalid),
        }
    }

    fn parent_is_dir(&self, p: &str) -> Result<(), FsError> {
        let (parent, _) = path::split(p).ok_or(FsError::Invalid)?;
        match self.nodes.get(parent) {
            Some(Node::Dir) => Ok(()),
            Some(Node::File(_)) => Err(FsError::NotDir),
            None => Err(FsError::NotFound),
        }
    }

    fn has_children(&self, p: &str) -> bool {
        let prefix = child_prefix(p);
        self.nodes
            .range(prefix.clone()..)
            .next()
            .is_some_and(|(k, _)| k.starts_with(&prefix) && k.len() > prefix.len())
    }
}

fn child_prefix(dir: &str) -> String {
    if dir == "/" {
        String::from("/")
    } else {
        let mut s = String::from(dir);
        s.push('/');
        s
    }
}

impl AppFs for MemFs {
    fn stat(&mut self, p: &str) -> Result<Stat, FsError> {
        let p = self.check(p)?;
        match self.nodes.get(p) {
            Some(Node::Dir) => Ok(Stat {
                kind: Kind::Dir,
                size: 0,
            }),
            Some(Node::File(d)) => Ok(Stat {
                kind: Kind::File,
                size: d.len() as u64,
            }),
            None => Err(FsError::NotFound),
        }
    }

    fn read_at(&mut self, p: &str, off: u64, buf: &mut [u8]) -> Result<usize, FsError> {
        let p = self.check(p)?;
        match self.nodes.get(p) {
            Some(Node::File(d)) => {
                if off >= d.len() as u64 {
                    return Ok(0);
                }
                let off = off as usize;
                let n = buf.len().min(d.len() - off);
                buf[..n].copy_from_slice(&d[off..off + n]);
                Ok(n)
            }
            Some(Node::Dir) => Err(FsError::IsDir),
            None => Err(FsError::NotFound),
        }
    }

    fn write_at(&mut self, p: &str, off: u64, data: &[u8]) -> Result<usize, FsError> {
        let p = self.check(p)?;
        let end = off.checked_add(data.len() as u64).ok_or(FsError::Invalid)?;
        if end > MAX_FILE {
            return Err(FsError::NoSpace);
        }
        let (max_bytes, bytes) = (self.max_bytes, self.bytes);
        match self.nodes.get_mut(p) {
            Some(Node::File(d)) => {
                let cur = d.len() as u64;
                let grow = end.saturating_sub(cur);
                if bytes + grow > max_bytes {
                    return Err(FsError::NoSpace);
                }
                if grow > 0 {
                    d.try_reserve(grow as usize).map_err(|_| FsError::NoSpace)?;
                    d.resize(end as usize, 0);
                }
                d[off as usize..end as usize].copy_from_slice(data);
                self.bytes += grow;
                Ok(data.len())
            }
            Some(Node::Dir) => Err(FsError::IsDir),
            None => Err(FsError::NotFound),
        }
    }

    fn set_len(&mut self, p: &str, len: u64) -> Result<(), FsError> {
        let p = self.check(p)?;
        if len > MAX_FILE {
            return Err(FsError::NoSpace);
        }
        let (max_bytes, bytes) = (self.max_bytes, self.bytes);
        match self.nodes.get_mut(p) {
            Some(Node::File(d)) => {
                let cur = d.len() as u64;
                if len > cur {
                    if bytes + (len - cur) > max_bytes {
                        return Err(FsError::NoSpace);
                    }
                    d.try_reserve((len - cur) as usize)
                        .map_err(|_| FsError::NoSpace)?;
                    d.resize(len as usize, 0);
                    self.bytes += len - cur;
                } else {
                    d.truncate(len as usize);
                    self.bytes -= cur - len;
                }
                Ok(())
            }
            Some(Node::Dir) => Err(FsError::IsDir),
            None => Err(FsError::NotFound),
        }
    }

    fn create(&mut self, p: &str) -> Result<(), FsError> {
        let p = self.check(p)?;
        if self.nodes.contains_key(p) {
            return Err(FsError::Exists);
        }
        self.parent_is_dir(p)?;
        if self.nodes.len() >= MAX_ENTRIES {
            return Err(FsError::NoSpace);
        }
        self.nodes.insert(String::from(p), Node::File(Vec::new()));
        Ok(())
    }

    fn mkdir(&mut self, p: &str) -> Result<(), FsError> {
        let p = self.check(p)?;
        if self.nodes.contains_key(p) {
            return Err(FsError::Exists);
        }
        self.parent_is_dir(p)?;
        if self.nodes.len() >= MAX_ENTRIES {
            return Err(FsError::NoSpace);
        }
        self.nodes.insert(String::from(p), Node::Dir);
        Ok(())
    }

    fn remove(&mut self, p: &str) -> Result<(), FsError> {
        let p = self.check(p)?;
        if p == "/" {
            return Err(FsError::Invalid);
        }
        match self.nodes.get(p) {
            None => return Err(FsError::NotFound),
            Some(Node::Dir) => {
                if self.has_children(p) {
                    return Err(FsError::NotEmpty);
                }
            }
            Some(Node::File(_)) => {}
        }
        if let Some(Node::File(d)) = self.nodes.remove(p) {
            self.bytes -= d.len() as u64;
        }
        Ok(())
    }

    fn rename(&mut self, from: &str, to: &str) -> Result<(), FsError> {
        let from = self.check(from)?;
        let to = self.check(to)?;
        if from == "/" || to == "/" {
            return Err(FsError::Invalid);
        }
        if !self.nodes.contains_key(from) {
            return Err(FsError::NotFound);
        }
        if self.nodes.contains_key(to) {
            return Err(FsError::Exists);
        }
        if within(to, from) {
            return Err(FsError::Invalid); // into its own subtree
        }
        self.parent_is_dir(to)?;
        let moving: Vec<String> = self
            .nodes
            .keys()
            .filter(|k| within(k, from))
            .cloned()
            .collect();
        for k in moving {
            let node = self.nodes.remove(&k).ok_or(FsError::Io)?;
            let mut nk = String::from(to);
            nk.push_str(&k[from.len()..]);
            self.nodes.insert(nk, node);
        }
        Ok(())
    }

    fn read_dir(&mut self, p: &str, index: usize) -> Result<Option<DirEntry>, FsError> {
        let p = self.check(p)?;
        match self.nodes.get(p) {
            Some(Node::Dir) => {}
            Some(Node::File(_)) => return Err(FsError::NotDir),
            None => return Err(FsError::NotFound),
        }
        let prefix = child_prefix(p);
        let entry = self
            .nodes
            .range(prefix.clone()..)
            .take_while(|(k, _)| k.starts_with(&prefix))
            .filter(|(k, _)| k.len() > prefix.len() && !k[prefix.len()..].contains('/'))
            .nth(index);
        Ok(entry.map(|(k, n)| DirEntry {
            name: String::from(&k[prefix.len()..]),
            kind: match n {
                Node::Dir => Kind::Dir,
                Node::File(_) => Kind::File,
            },
        }))
    }

    fn tree_size(&mut self, p: &str) -> Result<u64, FsError> {
        let p = self.check(p)?;
        if !self.nodes.contains_key(p) {
            return Err(FsError::NotFound);
        }
        let prefix = child_prefix(p);
        let mut total = 0u64;
        for (k, n) in self.nodes.range(prefix.clone()..) {
            if !k.starts_with(&prefix) {
                break;
            }
            if k.len() == prefix.len() {
                continue; // the root "/" itself
            }
            total += ENTRY_OVERHEAD
                + match n {
                    Node::File(d) => d.len() as u64,
                    Node::Dir => 0,
                };
        }
        Ok(total)
    }
}
