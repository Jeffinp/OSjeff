use super::*;
use alloc::vec;
use alloc::vec::Vec;

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

fn module(sections: &[(u8, &[u8])]) -> Vec<u8> {
    let mut v = HEADER.to_vec();
    for (id, payload) in sections {
        v.push(*id);
        leb(&mut v, payload.len() as u32);
        v.extend_from_slice(payload);
    }
    v
}

fn custom(name: &str, data: &[u8]) -> Vec<u8> {
    let mut p = Vec::new();
    leb(&mut p, name.len() as u32);
    p.extend_from_slice(name.as_bytes());
    p.extend_from_slice(data);
    p
}

#[test]
fn empty_module_has_no_sections() {
    let m = HEADER.to_vec();
    assert_eq!(Sections::new(&m).unwrap().count(), 0);
}

#[test]
fn short_inputs_are_rejected() {
    for n in 0..8 {
        assert_eq!(
            Sections::new(&HEADER[..n]).unwrap_err(),
            WasmError::TooShort
        );
    }
    assert_eq!(Sections::new(&[]).unwrap_err(), WasmError::TooShort);
}

#[test]
fn bad_magic_and_version() {
    let mut m = HEADER.to_vec();
    m[0] = 1;
    assert_eq!(Sections::new(&m).unwrap_err(), WasmError::BadMagic);
    let mut m = HEADER.to_vec();
    m[4] = 2;
    assert_eq!(Sections::new(&m).unwrap_err(), WasmError::BadVersion);
    let mut m = HEADER.to_vec();
    m[7] = 1;
    assert_eq!(Sections::new(&m).unwrap_err(), WasmError::BadVersion);
}

#[test]
fn leb_basic_values() {
    assert_eq!(read_leb_u32(&[0x00], 0), Ok((0, 1)));
    assert_eq!(read_leb_u32(&[0x7F], 0), Ok((127, 1)));
    assert_eq!(read_leb_u32(&[0x80, 0x01], 0), Ok((128, 2)));
    assert_eq!(read_leb_u32(&[0xE5, 0x8E, 0x26], 0), Ok((624485, 3)));
    assert_eq!(
        read_leb_u32(&[0xFF, 0xFF, 0xFF, 0xFF, 0x0F], 0),
        Ok((u32::MAX, 5))
    );
}

#[test]
fn leb_overlong_non_minimal_is_accepted_up_to_five_bytes() {
    assert_eq!(read_leb_u32(&[0x80, 0x80, 0x80, 0x80, 0x00], 0), Ok((0, 5)));
}

#[test]
fn leb_errors() {
    assert_eq!(
        read_leb_u32(&[0xFF, 0xFF, 0xFF, 0xFF, 0x1F], 0),
        Err(WasmError::BadLeb)
    );
    assert_eq!(
        read_leb_u32(&[0x80, 0x80, 0x80, 0x80, 0x80, 0x00], 0),
        Err(WasmError::BadLeb)
    );
    assert_eq!(read_leb_u32(&[0x80], 0), Err(WasmError::BadLeb));
    assert_eq!(read_leb_u32(&[], 0), Err(WasmError::BadLeb));
    assert_eq!(read_leb_u32(&[0x01], 5), Err(WasmError::BadLeb));
    assert_eq!(read_leb_u32(&[0x01], usize::MAX), Err(WasmError::BadLeb));
}

#[test]
fn walks_regular_and_custom_sections() {
    let c = custom("osjeff.manifest", b"id=x");
    let m = module(&[(1, &[0, 1, 2]), (0, &c), (10, &[9])]);
    let v: Vec<_> = Sections::new(&m).unwrap().map(|s| s.unwrap()).collect();
    assert_eq!(v.len(), 3);
    assert_eq!((v[0].id, v[0].data), (1, &[0u8, 1, 2][..]));
    assert_eq!(v[1].name, b"osjeff.manifest");
    assert_eq!(v[1].data, b"id=x");
    assert_eq!(v[2].id, 10);
}

#[test]
fn size_past_end_is_truncated() {
    let mut m = HEADER.to_vec();
    m.extend_from_slice(&[1, 5, 0, 0]);
    let mut it = Sections::new(&m).unwrap();
    assert_eq!(it.next(), Some(Err(WasmError::Truncated)));
    assert_eq!(it.next(), None, "iterator stops after an error");
}

#[test]
fn huge_size_does_not_overflow() {
    let mut m = HEADER.to_vec();
    m.extend_from_slice(&[1, 0xFF, 0xFF, 0xFF, 0xFF, 0x0F]);
    assert_eq!(
        Sections::new(&m).unwrap().next(),
        Some(Err(WasmError::Truncated))
    );
}

#[test]
fn bad_size_leb_in_section_header() {
    let mut m = HEADER.to_vec();
    m.extend_from_slice(&[1, 0x80]);
    assert_eq!(
        Sections::new(&m).unwrap().next(),
        Some(Err(WasmError::BadLeb))
    );
}

#[test]
fn section_id_above_13_is_rejected() {
    for id in [14u8, 15, 100, 255] {
        let m = module(&[(id, &[])]);
        assert_eq!(
            Sections::new(&m).unwrap().next(),
            Some(Err(WasmError::BadSectionId))
        );
    }
    let m = module(&[(13, &[])]);
    assert!(Sections::new(&m).unwrap().next().unwrap().is_ok());
}

#[test]
fn custom_name_must_fit_in_payload() {
    let m = module(&[(0, &[9, b'a', b'b'])]);
    assert_eq!(
        Sections::new(&m).unwrap().next(),
        Some(Err(WasmError::Truncated))
    );
    let m = module(&[(0, &[])]);
    assert_eq!(
        Sections::new(&m).unwrap().next(),
        Some(Err(WasmError::BadLeb))
    );
    let m = module(&[(0, &[0x80])]);
    assert_eq!(
        Sections::new(&m).unwrap().next(),
        Some(Err(WasmError::BadLeb))
    );
}

#[test]
fn custom_name_may_be_empty_and_non_utf8() {
    let m = module(&[(0, &[0, 1, 2]), (0, &[2, 0xFF, 0xFE, 7])]);
    let v: Vec<_> = Sections::new(&m).unwrap().map(|s| s.unwrap()).collect();
    assert_eq!(v[0].name, b"");
    assert_eq!(v[0].data, &[1, 2]);
    assert_eq!(v[1].name, &[0xFF, 0xFE]);
    assert_eq!(v[1].data, &[7]);
}

#[test]
fn too_many_sections() {
    let mut m = HEADER.to_vec();
    for _ in 0..MAX_SECTIONS + 1 {
        m.extend_from_slice(&[0, 1, 0]);
    }
    let n = Sections::new(&m).unwrap().filter(|s| s.is_ok()).count();
    assert_eq!(n, MAX_SECTIONS);
    assert_eq!(
        Sections::new(&m).unwrap().last(),
        Some(Err(WasmError::TooManySections))
    );
}

#[test]
fn find_custom_counts_duplicates() {
    let a = custom("osjeff.manifest", b"one");
    let b = custom("osjeff.manifest", b"two");
    let m = module(&[(0, &a), (0, &b)]);
    let (n, first) = find_custom(&m, b"osjeff.manifest").unwrap();
    assert_eq!(n, 2);
    assert_eq!(first, Some(&b"one"[..]));
    assert_eq!(find_custom(&m, b"nope").unwrap(), (0, None));
}

#[test]
fn find_custom_reports_malformed_tail() {
    let a = custom("osjeff.manifest", b"one");
    let mut m = module(&[(0, &a)]);
    m.extend_from_slice(&[1, 9]);
    assert_eq!(
        find_custom(&m, b"osjeff.manifest"),
        Err(WasmError::Truncated)
    );
}

#[test]
fn lookalike_inside_another_section_is_not_matched() {
    let c = custom("osjeff.manifest", b"id=x");
    let m = module(&[(1, &c)]);
    assert_eq!(find_custom(&m, b"osjeff.manifest").unwrap().0, 0);
}

#[test]
fn every_prefix_of_a_valid_module_is_handled() {
    let c = custom("osjeff.manifest", b"id=hello\nname=Hi\nversion=1.0.0\n");
    let m = module(&[(1, &[1, 2, 3]), (0, &c), (10, &[7, 7, 7])]);
    for n in 0..=m.len() {
        if let Ok(it) = Sections::new(&m[..n]) {
            for s in it {
                let _ = s;
            }
        }
    }
}

#[test]
fn single_byte_corruptions_never_panic() {
    let c = custom("osjeff.manifest", b"id=hello\n");
    let m = module(&[(1, &[1, 2, 3]), (0, &c), (10, &[7])]);
    for i in 0..m.len() {
        for b in [0x00u8, 0x7F, 0x80, 0xFF, 0x0F, 0x01] {
            let mut x = m.clone();
            x[i] = b;
            if let Ok(it) = Sections::new(&x) {
                for s in it {
                    let _ = s;
                }
            }
        }
    }
}

#[test]
fn display_messages_are_distinct() {
    use alloc::string::ToString;
    let all = [
        WasmError::TooShort,
        WasmError::BadMagic,
        WasmError::BadVersion,
        WasmError::BadLeb,
        WasmError::Truncated,
        WasmError::BadSectionId,
        WasmError::TooManySections,
    ];
    let mut seen = vec![];
    for e in all {
        let s = e.to_string();
        assert!(!seen.contains(&s));
        seen.push(s);
    }
}
