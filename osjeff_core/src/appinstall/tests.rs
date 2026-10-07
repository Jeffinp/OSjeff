use super::*;
use crate::appfs::MemFs;
use crate::appmanifest::{ICON_SECTION, MANIFEST_SECTION, ManifestError};
use crate::png;
use crate::wasmsec;

fn leb(v: &mut Vec<u8>, mut n: u32) {
    loop {
        let b = (n & 0x7F) as u8;
        n >>= 7;
        if n == 0 {
            v.push(b);
            return;
        }
        v.push(b | 0x80);
    }
}

fn section(name: &str, data: &[u8]) -> Vec<u8> {
    let mut p = Vec::new();
    leb(&mut p, name.len() as u32);
    p.extend_from_slice(name.as_bytes());
    p.extend_from_slice(data);
    let mut s = alloc::vec![0u8];
    leb(&mut s, p.len() as u32);
    s.extend_from_slice(&p);
    s
}

fn pkg(manifest: &str) -> Vec<u8> {
    let mut w = wasmsec::HEADER.to_vec();
    w.extend_from_slice(&section(MANIFEST_SECTION, manifest.as_bytes()));
    w
}

fn app(id: &str, name: &str) -> Vec<u8> {
    pkg(&alloc::format!("id={id}\nname={name}\nversion=1.0.0\n"))
}

fn icon_png(side: usize, color: u32) -> Vec<u8> {
    let img = Image::from_pixels(side, side, alloc::vec![color; side * side]).unwrap();
    png::encode(&img).unwrap()
}

fn fs() -> MemFs {
    MemFs::new(16 << 20)
}

#[test]
fn install_then_list_then_read_back() {
    let mut f = fs();
    let w = app("clock", "Clock");
    let m = install(&mut f, &w).unwrap();
    assert_eq!(m.id, "clock");
    assert!(is_installed(&mut f, "clock"));
    assert_eq!(installed_ids(&mut f).unwrap(), ["clock"]);
    assert_eq!(read_package(&mut f, "clock").unwrap(), w);
}

#[test]
fn duplicate_id_is_refused_and_original_kept() {
    let mut f = fs();
    let a = app("dup", "First");
    install(&mut f, &a).unwrap();
    let b = app("dup", "Second");
    assert_eq!(install(&mut f, &b).unwrap_err(), InstallError::Duplicate);
    assert_eq!(read_package(&mut f, "dup").unwrap(), a);
}

#[test]
fn invalid_manifest_is_refused_without_writing() {
    let mut f = fs();
    let bad = pkg("id=x\nname=X\nversion=1.0.0\nfs=everything\n");
    assert_eq!(
        install(&mut f, &bad).unwrap_err(),
        InstallError::Package(PackageError::Manifest(ManifestError::BadValue("fs")))
    );
    assert!(installed_ids(&mut f).unwrap().is_empty());
    assert!(f.stat("/apps/.install.tmp").is_err());
}

#[test]
fn quota_above_the_ceiling_is_refused() {
    let mut f = fs();
    let bad = pkg("id=big\nname=Big\nversion=1.0.0\nmem_mib=512\n");
    assert_eq!(
        install(&mut f, &bad).unwrap_err(),
        InstallError::Package(PackageError::Manifest(ManifestError::OverLimit("mem_mib")))
    );
    let bad = pkg("id=big\nname=Big\nversion=1.0.0\nfuel_frame=999999999\n");
    assert!(matches!(
        install(&mut f, &bad),
        Err(InstallError::Package(PackageError::Manifest(
            ManifestError::OverLimit("fuel_frame")
        )))
    ));
}

#[test]
fn garbage_and_missing_manifest() {
    let mut f = fs();
    assert!(matches!(
        install(&mut f, b"hello"),
        Err(InstallError::Package(PackageError::Wasm(_)))
    ));
    assert_eq!(
        install(&mut f, &wasmsec::HEADER).unwrap_err(),
        InstallError::Package(PackageError::NoManifest)
    );
    assert!(installed_ids(&mut f).unwrap().is_empty());
}

#[test]
fn oversize_package() {
    let mut f = fs();
    let mut w = app("huge", "Huge");
    // a data-ish padding custom section pushing it over the limit
    w.extend_from_slice(&section("pad", &alloc::vec![0u8; MAX_PACKAGE_BYTES]));
    assert_eq!(install(&mut f, &w).unwrap_err(), InstallError::TooLarge);
}

#[test]
fn remove_and_reinstall() {
    let mut f = fs();
    install(&mut f, &app("gone", "Gone")).unwrap();
    remove(&mut f, "gone").unwrap();
    assert!(!is_installed(&mut f, "gone"));
    assert_eq!(
        remove(&mut f, "gone").unwrap_err(),
        InstallError::NotInstalled
    );
    assert_eq!(remove(&mut f, "../x").unwrap_err(), InstallError::BadId);
    assert_eq!(remove(&mut f, "").unwrap_err(), InstallError::BadId);
    install(&mut f, &app("gone", "Gone")).unwrap();
}

#[test]
fn remove_keeps_app_data() {
    let mut f = fs();
    install(&mut f, &app("keep", "Keep")).unwrap();
    f.mkdir_all("/data/keep").unwrap();
    f.create("/data/keep/notes").unwrap();
    remove(&mut f, "keep").unwrap();
    assert!(f.stat("/data/keep/notes").is_ok());
}

#[test]
fn max_id_length_installs() {
    let mut f = fs();
    let id = "a".repeat(32);
    install(&mut f, &app(&id, "Long")).unwrap();
    assert!(is_installed(&mut f, &id));
    assert_eq!(installed_ids(&mut f).unwrap(), std::slice::from_ref(&id));
    remove(&mut f, &id).unwrap();
}

#[test]
fn catalog_lists_name_sorted_with_icons() {
    let mut f = fs();
    install(&mut f, &app("zeta", "Zeta")).unwrap();
    let mut with_icon = app("alpha", "alpha");
    with_icon.extend_from_slice(&section(ICON_SECTION, &icon_png(48, 0xFF112233)));
    install(&mut f, &with_icon).unwrap();
    install(&mut f, &app("mid", "Mid")).unwrap();
    let cat = load_catalog(&mut f);
    let names: Vec<_> = cat.iter().map(|c| c.manifest.name.as_str()).collect();
    assert_eq!(names, ["alpha", "Mid", "Zeta"]);
    let icon = cat[0].icon.as_ref().unwrap();
    assert_eq!(icon.len(), ICON_SIZE * ICON_SIZE);
    assert_eq!(
        icon[0], 0xFF112233,
        "a constant icon scales to the same color"
    );
    assert!(cat[1].icon.is_none());
}

#[test]
fn catalog_skips_corrupt_or_mismatched_files() {
    let mut f = fs();
    install(&mut f, &app("ok", "Ok")).unwrap();
    // a file dropped into /apps by hand: wrong id for its name, and garbage
    f.create("/apps/liar.wasm").unwrap();
    f.write_at("/apps/liar.wasm", 0, &app("other", "Other"))
        .unwrap();
    f.create("/apps/junk.wasm").unwrap();
    f.write_at("/apps/junk.wasm", 0, b"not wasm").unwrap();
    f.create("/apps/readme.txt").unwrap();
    f.mkdir("/apps/dir.wasm").unwrap();
    let cat = load_catalog(&mut f);
    assert_eq!(cat.len(), 1);
    assert_eq!(cat[0].manifest.id, "ok");
    let ids = installed_ids(&mut f).unwrap();
    assert_eq!(ids, ["junk", "liar", "ok"]);
}

#[test]
fn catalog_of_a_fresh_system_is_empty() {
    let mut f = fs();
    assert!(load_catalog(&mut f).is_empty());
    assert!(installed_ids(&mut f).unwrap().is_empty());
}

#[test]
fn seed_installs_missing_and_never_overwrites() {
    let mut f = fs();
    let a = app("one", "One");
    let b = app("two", "Two");
    assert_eq!(seed(&mut f, &[&a, &b]), 2);
    // the user replaced `one` by their own build; reseeding keeps it
    remove(&mut f, "one").unwrap();
    let mine = app("one", "My One");
    install(&mut f, &mine).unwrap();
    assert_eq!(seed(&mut f, &[&a, &b]), 0);
    assert_eq!(read_package(&mut f, "one").unwrap(), mine);
    // an invalid bundled package is skipped, the rest installs
    let c = app("three", "Three");
    assert_eq!(seed(&mut f, &[b"junk", &c]), 1);
}

#[test]
fn catalog_limit() {
    let mut f = fs();
    for i in 0..MAX_APPS {
        install(&mut f, &app(&alloc::format!("a{i}"), "A")).unwrap();
    }
    assert_eq!(
        install(&mut f, &app("onetoomany", "X")).unwrap_err(),
        InstallError::Full
    );
}

#[test]
fn leftover_temp_file_is_cleaned_by_the_next_install() {
    let mut f = fs();
    f.mkdir_all("/apps").unwrap();
    f.create("/apps/.install.tmp").unwrap();
    f.write_at("/apps/.install.tmp", 0, b"stale").unwrap();
    install(&mut f, &app("fresh", "Fresh")).unwrap();
    assert!(f.stat("/apps/.install.tmp").is_err());
    assert_eq!(installed_ids(&mut f).unwrap(), ["fresh"]);
}

#[test]
fn failed_write_leaves_nothing_behind() {
    let mut f = MemFs::new(40); // far too small for a package
    let w = app("tiny", "Tiny");
    assert!(matches!(install(&mut f, &w), Err(InstallError::Fs(_))));
    assert!(installed_ids(&mut f).unwrap().is_empty());
    assert!(f.stat("/apps/.install.tmp").is_err());
}

#[test]
fn read_package_errors() {
    let mut f = fs();
    assert_eq!(
        read_package(&mut f, "nope").unwrap_err(),
        InstallError::NotInstalled
    );
    assert_eq!(
        read_package(&mut f, "../etc").unwrap_err(),
        InstallError::BadId
    );
}

#[test]
fn error_messages() {
    use alloc::string::ToString;
    for e in [
        InstallError::TooLarge,
        InstallError::Duplicate,
        InstallError::NotInstalled,
        InstallError::BadId,
        InstallError::Full,
        InstallError::Fs(FsError::NoSpace),
        InstallError::Package(PackageError::NoManifest),
    ] {
        assert!(!e.to_string().is_empty());
    }
}

#[test]
fn seed_once_offers_each_package_exactly_once() {
    let mut f = fs();
    let (a, b) = (app("alpha", "Alpha"), app("beta", "Beta"));
    let bundled: [&[u8]; 2] = [&a, &b];
    assert_eq!(seed_once(&mut f, &bundled), 2);
    assert_eq!(installed_ids(&mut f).unwrap(), ["alpha", "beta"]);
    // A second boot installs nothing.
    assert_eq!(seed_once(&mut f, &bundled), 0);
    // The user removes one: it must NOT come back at the next boot.
    remove(&mut f, "alpha").unwrap();
    assert_eq!(seed_once(&mut f, &bundled), 0);
    assert_eq!(installed_ids(&mut f).unwrap(), ["beta"]);
    // The stateless seed would have resurrected it.
    assert_eq!(seed(&mut f, &bundled), 1);
}

#[test]
fn seed_once_installs_packages_that_are_new_in_a_later_build() {
    let mut f = fs();
    let (a, b) = (app("alpha", "Alpha"), app("beta", "Beta"));
    assert_eq!(seed_once(&mut f, &[&a]), 1);
    remove(&mut f, "alpha").unwrap();
    // The OS was updated and now bundles `beta` too.
    assert_eq!(seed_once(&mut f, &[&a, &b]), 1);
    assert_eq!(installed_ids(&mut f).unwrap(), ["beta"]);
}

#[test]
fn seed_once_counts_a_user_installed_package_as_offered() {
    let mut f = fs();
    let a = app("alpha", "Alpha");
    install(&mut f, &a).unwrap();
    assert_eq!(seed_once(&mut f, &[&a]), 0);
    remove(&mut f, "alpha").unwrap();
    assert_eq!(
        seed_once(&mut f, &[&a]),
        0,
        "removal after that stays removed"
    );
}

#[test]
fn seed_once_retries_what_failed_and_ignores_a_bad_marker() {
    // No room: nothing installed, nothing marked.
    let mut tiny = MemFs::new(8);
    let a = app("alpha", "Alpha");
    assert_eq!(seed_once(&mut tiny, &[&a]), 0);
    assert!(tiny.stat("/apps/.seeded").is_err());
    // A marker full of junk (hostile ids, too long, binary) offers nothing it should not.
    let mut f = fs();
    f.mkdir_all("/apps").unwrap();
    f.create("/apps/.seeded").unwrap();
    f.write_at(
        "/apps/.seeded",
        0,
        b"../../etc\n\xff\xfe\nALPHA\nbeta\nbeta\n",
    )
    .unwrap();
    let b = app("beta", "Beta");
    assert_eq!(seed_once(&mut f, &[&a, &b]), 1);
    assert_eq!(installed_ids(&mut f).unwrap(), ["alpha"]);
}

#[test]
fn the_seed_marker_is_not_an_app_and_survives_catalog_walks() {
    let mut f = fs();
    let a = app("alpha", "Alpha");
    seed_once(&mut f, &[&a]);
    assert_eq!(installed_ids(&mut f).unwrap(), ["alpha"]);
    assert_eq!(load_catalog(&mut f).len(), 1);
    assert_eq!(f.stat("/apps/.seeded").unwrap().kind, Kind::File);
}
