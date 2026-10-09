use super::*;

#[derive(Default)]
struct FakeVol(alloc::collections::BTreeMap<Vec<u8>, Vec<u8>>);
impl ConfFiles for FakeVol {
    fn read(&mut self, p: &[u8]) -> Option<Vec<u8>> {
        self.0.get(p).cloned()
    }
    fn write(&mut self, p: &[u8], d: &[u8]) -> Result<(), SinkError> {
        self.0.insert(p.to_vec(), d.to_vec());
        Ok(())
    }
    fn remove(&mut self, p: &[u8]) {
        self.0.remove(p);
    }
}

#[test]
fn settings_fall_back_to_the_old_file_and_migrate_on_save() {
    let mut v = FakeVol::default();
    v.0.insert(
        LEGACY_SETTINGS_PATH.to_vec(),
        b"# OSjeff settings\nversion=1\n".to_vec(),
    );
    let mut st = MigratingStore(v);
    assert_eq!(st.load().unwrap(), b"# OSjeff settings\nversion=1\n");
    st.save(b"new").unwrap();
    assert_eq!(st.0.0.get(SETTINGS_PATH).unwrap(), b"new");
    assert!(!st.0.0.contains_key(LEGACY_SETTINGS_PATH));
    assert_eq!(st.load().unwrap(), b"new");
}

#[test]
fn settings_new_file_wins_over_the_old_one() {
    let mut v = FakeVol::default();
    v.0.insert(LEGACY_SETTINGS_PATH.to_vec(), b"old".to_vec());
    v.0.insert(SETTINGS_PATH.to_vec(), b"new".to_vec());
    assert_eq!(MigratingStore(v).load().unwrap(), b"new");
    assert_eq!(MigratingStore(FakeVol::default()).load(), None);
}

#[test]
fn permille() {
    let u = DiskUsageInfo {
        total_bytes: Some(2000),
        used_bytes: Some(500),
        ..Default::default()
    };
    assert_eq!(u.used_permille(), Some(250));
    assert_eq!(DiskUsageInfo::default().used_permille(), None);
    let z = DiskUsageInfo {
        total_bytes: Some(0),
        used_bytes: Some(0),
        ..Default::default()
    };
    assert_eq!(z.used_permille(), None);
    let over = DiskUsageInfo {
        total_bytes: Some(10),
        used_bytes: Some(99),
        ..Default::default()
    };
    assert_eq!(over.used_permille(), Some(1000));
}

#[test]
fn defaults_behave() {
    assert_eq!(NoNetControl.renew_dhcp(), Err(NetControlError::Unsupported));
    let mut s = MemSink::default();
    s.write_file(b"a.log", b"x").unwrap();
    assert_eq!(
        (s.name.as_slice(), s.data.as_slice()),
        (&b"a.log"[..], &b"x"[..])
    );
    let mut st = MemStore::default();
    assert!(st.load().is_none());
    st.save(b"k=v").unwrap();
    assert_eq!(st.load().unwrap(), b"k=v");
}
