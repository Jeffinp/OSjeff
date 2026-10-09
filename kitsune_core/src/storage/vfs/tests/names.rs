use super::*;

#[test]
fn names_are_validated() {
    assert_eq!(validate_name(b"ok.txt"), Ok(()));
    assert_eq!(validate_name(b""), Err(VfsError::InvalidName));
    assert_eq!(validate_name(b"."), Err(VfsError::InvalidName));
    assert_eq!(validate_name(b".."), Err(VfsError::InvalidName));
    assert_eq!(validate_name(b"a/b"), Err(VfsError::InvalidName));
    assert_eq!(validate_name(b"a\0b"), Err(VfsError::InvalidName));
    assert_eq!(validate_name(b"a\nb"), Err(VfsError::InvalidName));
    assert_eq!(validate_name(&[0xFF, 0xFE]), Err(VfsError::InvalidName));
    assert_eq!(validate_name(&[b'a'; 256]), Err(VfsError::NameTooLong));
    assert_eq!(validate_name(&[b'a'; 255]), Ok(()));
}

#[test]
fn utf8_names_are_valid() {
    assert_eq!(validate_name("relatório ção.txt".as_bytes()), Ok(()));
    assert_eq!(validate_name("日本語.png".as_bytes()), Ok(()));
}

#[test]
fn trim_name_strips_spaces_only_at_the_ends() {
    assert_eq!(trim_name(b"  a b  "), b"a b");
    assert_eq!(trim_name(b"   "), b"");
}

#[test]
fn split_ext_cases() {
    assert_eq!(split_ext(b"a.txt"), (&b"a"[..], &b".txt"[..]));
    assert_eq!(split_ext(b"a.tar.gz"), (&b"a.tar"[..], &b".gz"[..]));
    assert_eq!(split_ext(b".profile"), (&b".profile"[..], &b""[..]));
    assert_eq!(split_ext(b"end."), (&b"end."[..], &b""[..]));
    assert_eq!(split_ext(b"plain"), (&b"plain"[..], &b""[..]));
}

#[test]
fn unique_name_keeps_a_free_name() {
    assert_eq!(unique_name(b"a.txt", |_| false), b"a.txt");
}

#[test]
fn unique_name_adds_a_counter_before_the_extension() {
    assert_eq!(unique_name(b"a.txt", |n| n == b"a.txt"), b"a (2).txt");
    assert_eq!(
        unique_name(b"a.txt", |n| n == b"a.txt" || n == b"a (2).txt"),
        b"a (3).txt"
    );
}

#[test]
fn unique_name_replaces_an_existing_counter() {
    assert_eq!(
        unique_name(b"a (2).txt", |n| n == b"a (2).txt"),
        b"a (3).txt".to_vec()
    );
    // A non-numeric suffix is part of the name.
    assert_eq!(
        unique_name(b"a (x).txt", |n| n == b"a (x).txt"),
        b"a (x) (2).txt".to_vec()
    );
}

#[test]
fn unique_name_for_a_folder_and_a_dotfile() {
    assert_eq!(unique_name(b"Docs", |n| n == b"Docs"), b"Docs (2)");
    assert_eq!(unique_name(b".rc", |n| n == b".rc"), b".rc (2)");
}

#[test]
fn unique_name_never_exceeds_255_bytes_and_stays_utf8() {
    let long: String = "é".repeat(127); // 254 bytes
    let name = long.as_bytes().to_vec();
    let got = unique_name(&name, |n| n == &name[..]);
    assert!(got.len() <= 255);
    assert!(core::str::from_utf8(&got).is_ok());
    assert!(got.ends_with(b" (2)"));
    let mut full = vec![b'a'; 255];
    full[250..].copy_from_slice(b".abcd");
    let got = unique_name(&full, |n| n == &full[..]);
    assert!(got.len() <= 255);
    assert!(got.ends_with(b" (2).abcd"));
}
