//! Deterministic property test: thousands of random operations are applied to
//! the real filesystem and to a trivial in-memory model, and every outcome
//! (success or failure) and the whole tree are compared; `fsck` runs every N
//! steps and the filesystem is periodically unmounted and mounted again.

use super::*;
use alloc::collections::BTreeMap;
use alloc::string::String;

#[derive(Clone, Debug, PartialEq, Eq)]
enum N {
    File(Vec<u8>),
    /// Directory with a unique identity (inodes get reused, ids do not).
    Dir(u32),
}

#[derive(Clone, Debug)]
struct TrashItem {
    orig_parent_id: u32,
    orig_name: String,
    /// The item itself (`""`) and, for a directory, everything below it
    /// (`"/x/y"` relative paths).
    tree: BTreeMap<String, N>,
}

#[derive(Default)]
struct Model {
    live: BTreeMap<String, N>,
    trash: BTreeMap<String, TrashItem>,
    next_id: u32,
}

fn split(p: &str) -> (String, String) {
    let i = p.rfind('/').unwrap();
    (p[..i].to_string(), p[i + 1..].to_string())
}

use alloc::string::ToString;

impl Model {
    fn new() -> Model {
        Model {
            next_id: 1,
            ..Default::default()
        }
    }
    fn dir_ok(&self, path: &str) -> bool {
        path.is_empty() || matches!(self.live.get(path), Some(N::Dir(_)))
    }
    fn id_of(&self, path: &str) -> u32 {
        if path.is_empty() {
            0
        } else if let Some(N::Dir(i)) = self.live.get(path) {
            *i
        } else {
            u32::MAX
        }
    }
    fn path_of_id(&self, id: u32) -> Option<String> {
        if id == 0 {
            return Some(String::new());
        }
        self.live
            .iter()
            .find(|(_, n)| **n == N::Dir(id))
            .map(|(p, _)| p.clone())
    }
    fn subtree(&self, path: &str) -> BTreeMap<String, N> {
        let mut out = BTreeMap::new();
        let prefix = alloc::format!("{path}/");
        for (p, n) in &self.live {
            if p == path {
                out.insert(String::new(), n.clone());
            } else if let Some(rest) = p.strip_prefix(&prefix) {
                out.insert(alloc::format!("/{rest}"), n.clone());
            }
        }
        out
    }
    fn drop_subtree(&mut self, path: &str) {
        let prefix = alloc::format!("{path}/");
        self.live
            .retain(|p, _| p != path && !p.starts_with(&prefix));
    }
    fn is_empty_dir(&self, path: &str) -> bool {
        let prefix = alloc::format!("{path}/");
        !self.live.keys().any(|p| p.starts_with(&prefix))
    }

    fn new_node(&mut self, path: &str, dir: bool) -> Result<(), ()> {
        let (parent, _) = split(path);
        if !self.dir_ok(&parent) || self.live.contains_key(path) {
            return Err(());
        }
        let n = if dir {
            self.next_id += 1;
            N::Dir(self.next_id)
        } else {
            N::File(Vec::new())
        };
        self.live.insert(path.to_string(), n);
        Ok(())
    }
    fn write_file(&mut self, path: &str, data: &[u8]) -> Result<(), ()> {
        let (parent, _) = split(path);
        if !self.dir_ok(&parent) {
            return Err(());
        }
        match self.live.get_mut(path) {
            Some(N::File(c)) => *c = data.to_vec(),
            Some(N::Dir(_)) => return Err(()),
            None => {
                self.live.insert(path.to_string(), N::File(data.to_vec()));
            }
        }
        Ok(())
    }
    fn file_mut(&mut self, path: &str) -> Result<&mut Vec<u8>, ()> {
        match self.live.get_mut(path) {
            Some(N::File(c)) => Ok(c),
            _ => Err(()),
        }
    }
    fn rename(&mut self, a: &str, b: &str) -> Result<(), ()> {
        if !self.live.contains_key(a) {
            return Err(());
        }
        if a == b {
            return Ok(());
        }
        let (bp, _) = split(b);
        if !self.dir_ok(&bp) || self.live.contains_key(b) {
            return Err(());
        }
        if matches!(self.live.get(a), Some(N::Dir(_))) && (b.starts_with(&alloc::format!("{a}/"))) {
            return Err(());
        }
        let sub = self.subtree(a);
        self.drop_subtree(a);
        for (rel, n) in sub {
            self.live.insert(alloc::format!("{b}{rel}"), n);
        }
        Ok(())
    }
    fn trash(&mut self, path: &str) -> Result<(), ()> {
        if !self.live.contains_key(path) {
            return Err(());
        }
        let (parent, name) = split(path);
        let mut tname = name.clone();
        let mut n = 2;
        while self.trash.contains_key(&tname) {
            tname = alloc::format!("{name}~{n}");
            n += 1;
        }
        let item = TrashItem {
            orig_parent_id: self.id_of(&parent),
            orig_name: name,
            tree: self.subtree(path),
        };
        self.drop_subtree(path);
        self.trash.insert(tname, item);
        Ok(())
    }
    fn restore(&mut self, tname: &str) -> Result<(), ()> {
        let item = self.trash.get(tname).ok_or(())?.clone();
        let dest_parent = self.path_of_id(item.orig_parent_id).unwrap_or_default();
        let dest = alloc::format!("{dest_parent}/{}", item.orig_name);
        if self.live.contains_key(&dest) {
            return Err(());
        }
        self.trash.remove(tname);
        for (rel, n) in item.tree {
            self.live.insert(alloc::format!("{dest}{rel}"), n);
        }
        Ok(())
    }

    /// Flatten to the same shape the filesystem snapshot has.
    fn flatten(&self) -> BTreeMap<Vec<u8>, Option<Vec<u8>>> {
        let mut m = BTreeMap::new();
        m.insert(b"/.trash".to_vec(), None);
        for (p, n) in &self.live {
            m.insert(
                p.as_bytes().to_vec(),
                match n {
                    N::File(c) => Some(c.clone()),
                    N::Dir(_) => None,
                },
            );
        }
        for (t, item) in &self.trash {
            for (rel, n) in &item.tree {
                m.insert(
                    alloc::format!("/.trash/{t}{rel}").into_bytes(),
                    match n {
                        N::File(c) => Some(c.clone()),
                        N::Dir(_) => None,
                    },
                );
            }
        }
        m
    }
}

const NAMES: [&str; 6] = ["a", "b", "c", "d", "e", "f"];

fn pick_path(rng: &mut Rng, m: &Model, existing_bias: u64) -> String {
    if !m.live.is_empty() && rng.below(100) < existing_bias {
        let i = rng.below(m.live.len() as u64) as usize;
        return m.live.keys().nth(i).unwrap().clone();
    }
    let depth = 1 + rng.below(3) as usize;
    let mut p = String::new();
    for _ in 0..depth {
        p.push('/');
        p.push_str(NAMES[rng.below(NAMES.len() as u64) as usize]);
    }
    p
}

fn pick_data(rng: &mut Rng) -> Vec<u8> {
    let n = match rng.below(10) {
        0..=6 => rng.below(300),
        7..=8 => rng.below(10_000),
        _ => rng.below(40_000),
    } as usize;
    let b = rng.next() as u8;
    // Compressible-ish but varied content so block mix-ups are noticed.
    (0..n).map(|i| b.wrapping_add((i / 7) as u8)).collect()
}

fn run(seed: u64, steps: u32, remount_every: u32, check_every: u32) {
    let mut fs = fresh_with_inodes(8, 4096);
    let mut m = Model::new();
    let mut rng = Rng(seed);
    let mut counts = [0u32; 14];
    let mut ok_counts = [0u32; 14];
    for step in 0..steps {
        let now = 1_000_000 + step as u64;
        let kind = rng.below(14) as usize;
        counts[kind] += 1;
        let (model_res, fs_res): (Result<(), ()>, Result<(), FsError>);
        let label: String;
        match kind {
            0 => {
                let p = pick_path(&mut rng, &m, 20);
                label = alloc::format!("create {p}");
                model_res = m.new_node(&p, false);
                fs_res = fs.create(&p, now).map(|_| ());
            }
            1 => {
                let p = pick_path(&mut rng, &m, 20);
                label = alloc::format!("mkdir {p}");
                model_res = m.new_node(&p, true);
                fs_res = fs.mkdir(&p, now).map(|_| ());
            }
            2 | 3 => {
                let p = pick_path(&mut rng, &m, 50);
                let d = pick_data(&mut rng);
                label = alloc::format!("write_file {p} {}", d.len());
                model_res = m.write_file(&p, &d);
                fs_res = fs.write_file(&p, &d, now);
            }
            4 => {
                let p = pick_path(&mut rng, &m, 90);
                let off = rng.below(20_000);
                let d = pick_data(&mut rng);
                label = alloc::format!("write_at {p} {off} {}", d.len());
                model_res = match m.file_mut(&p) {
                    Ok(c) => {
                        let off = off as usize;
                        if !d.is_empty() {
                            if c.len() < off + d.len() {
                                c.resize(off + d.len(), 0);
                            }
                            c[off..off + d.len()].copy_from_slice(&d);
                        }
                        Ok(())
                    }
                    Err(()) => Err(()),
                };
                fs_res = fs.lookup(&p).and_then(|i| fs.write_at(i, off, &d, now));
            }
            5 => {
                let p = pick_path(&mut rng, &m, 90);
                let d = pick_data(&mut rng);
                label = alloc::format!("append {p} {}", d.len());
                model_res = m.file_mut(&p).map(|c| c.extend_from_slice(&d));
                fs_res = fs.lookup(&p).and_then(|i| fs.append(i, &d, now));
            }
            6 => {
                let p = pick_path(&mut rng, &m, 90);
                let n = rng.below(30_000);
                label = alloc::format!("truncate {p} {n}");
                model_res = m.file_mut(&p).map(|c| c.resize(n as usize, 0));
                fs_res = fs.lookup(&p).and_then(|i| fs.truncate(i, n, now));
            }
            7 => {
                let a = pick_path(&mut rng, &m, 85);
                let b = pick_path(&mut rng, &m, 10);
                label = alloc::format!("rename {a} {b}");
                model_res = m.rename(&a, &b);
                fs_res = fs.rename(&a, &b, now);
            }
            8 => {
                let p = pick_path(&mut rng, &m, 90);
                label = alloc::format!("remove {p}");
                model_res = match m.live.get(&p) {
                    Some(N::File(_)) => {
                        m.live.remove(&p);
                        Ok(())
                    }
                    _ => Err(()),
                };
                fs_res = fs.remove(&p);
            }
            9 => {
                let p = pick_path(&mut rng, &m, 90);
                label = alloc::format!("rmdir {p}");
                model_res = match m.live.get(&p) {
                    Some(N::Dir(_)) if m.is_empty_dir(&p) => {
                        m.live.remove(&p);
                        Ok(())
                    }
                    _ => Err(()),
                };
                fs_res = fs.rmdir(&p);
            }
            10 => {
                let p = pick_path(&mut rng, &m, 90);
                label = alloc::format!("remove_all {p}");
                model_res = if m.live.contains_key(&p) {
                    m.drop_subtree(&p);
                    Ok(())
                } else {
                    Err(())
                };
                fs_res = fs.remove_all(&p);
            }
            11 => {
                let p = pick_path(&mut rng, &m, 90);
                label = alloc::format!("trash {p}");
                model_res = m.trash(&p);
                fs_res = fs.trash(&p, now);
            }
            12 => {
                let names: Vec<String> = m.trash.keys().cloned().collect();
                let t = if names.is_empty() || rng.below(10) == 0 {
                    "nothing".to_string()
                } else {
                    names[rng.below(names.len() as u64) as usize].clone()
                };
                label = alloc::format!("restore {t}");
                model_res = m.restore(&t);
                fs_res = fs.trash_restore(t.as_bytes(), now).map(|_| ());
            }
            _ => {
                let names: Vec<String> = m.trash.keys().cloned().collect();
                if rng.below(8) == 0 {
                    label = "empty_trash".to_string();
                    m.trash.clear();
                    model_res = Ok(());
                    fs_res = fs.empty_trash();
                } else {
                    let t = if names.is_empty() {
                        "nothing".to_string()
                    } else {
                        names[rng.below(names.len() as u64) as usize].clone()
                    };
                    label = alloc::format!("purge {t}");
                    model_res = if m.trash.remove(&t).is_some() {
                        Ok(())
                    } else {
                        Err(())
                    };
                    fs_res = fs.trash_purge(t.as_bytes());
                }
            }
        }
        ok_counts[kind] += model_res.is_ok() as u32;
        assert_eq!(
            model_res.is_ok(),
            fs_res.is_ok(),
            "seed {seed} step {step}: {label}: model {model_res:?} fs {fs_res:?}"
        );
        if (step + 1) % check_every == 0 {
            let r = fs.fsck().unwrap();
            assert!(r.is_clean(), "seed {seed} step {step} after {label}: {r:?}");
            assert_eq!(
                super::crash::snapshot(&mut fs),
                m.flatten(),
                "seed {seed} step {step} after {label}"
            );
        }
        if (step + 1) % remount_every == 0 {
            fs = Fs3::mount_verified(fs.into_device()).unwrap();
            assert_eq!(
                super::crash::snapshot(&mut fs),
                m.flatten(),
                "after remount at {step}"
            );
        }
    }
    let r = fs.fsck().unwrap();
    assert!(r.is_clean(), "seed {seed} final: {r:?}");
    assert_eq!(
        super::crash::snapshot(&mut fs),
        m.flatten(),
        "seed {seed} final"
    );
    // Every kind of operation was really exercised.
    assert!(counts.iter().all(|&c| c > steps / 40), "{counts:?}");
    // ...and a good share of each actually succeeded (not just error paths).
    for (k, (&ok, &n)) in ok_counts.iter().zip(counts.iter()).enumerate() {
        let min_pct = if matches!(k, 7 | 9 | 12 | 13) { 3 } else { 10 };
        assert!(ok * 100 > n * min_pct, "kind {k}: {ok}/{n} succeeded");
    }
}

#[test]
fn random_operations_match_the_model_seed_1() {
    run(1, 6000, 400, 60);
}

#[test]
fn random_operations_match_the_model_seed_2() {
    run(0xC0FFEE, 6000, 333, 50);
}

#[test]
fn random_operations_match_the_model_seed_3() {
    run(987_654_321, 6000, 500, 100);
}

#[test]
fn random_operations_match_the_model_with_a_tiny_cache() {
    // Same property, but every access evicts something.
    let mut fs = Fs3::mount_with(fresh_with_inodes(8, 4096).into_device(), 3).unwrap();
    let mut m = Model::new();
    let mut rng = Rng(4242);
    for step in 0..1200u32 {
        let now = 2_000_000 + step as u64;
        let p = pick_path(&mut rng, &m, 40);
        match rng.below(4) {
            0 => {
                let d = pick_data(&mut rng);
                assert_eq!(
                    m.write_file(&p, &d).is_ok(),
                    fs.write_file(&p, &d, now).is_ok()
                );
            }
            1 => assert_eq!(m.new_node(&p, true).is_ok(), fs.mkdir(&p, now).is_ok()),
            2 => {
                assert_eq!(m.trash(&p).is_ok(), fs.trash(&p, now).is_ok());
            }
            _ => {
                let b = pick_path(&mut rng, &m, 10);
                assert_eq!(m.rename(&p, &b).is_ok(), fs.rename(&p, &b, now).is_ok());
            }
        }
    }
    assert_clean(&mut fs);
    assert_eq!(super::crash::snapshot(&mut fs), m.flatten());
}

#[test]
fn model_sanity_itself() {
    // Guard the oracle: the model must reject what the filesystem rejects.
    let mut m = Model::new();
    assert!(m.new_node("/a", true).is_ok());
    assert!(m.new_node("/a", true).is_err());
    assert!(m.new_node("/x/y", false).is_err());
    assert!(m.rename("/a", "/a/b").is_err());
    assert!(m.write_file("/a", b"x").is_err());
    assert!(m.trash("/a").is_ok());
    assert!(m.restore("a").is_ok());
    assert!(m.restore("a").is_err());
}

/// A wider sweep for local use: `cargo test -p osjeff_core -- --ignored many_seeds`.
#[test]
#[ignore = "slow sweep; run with --ignored"]
fn random_operations_match_the_model_many_seeds() {
    for seed in 1..=60u64 {
        run(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15), 6000, 700, 150);
    }
}
