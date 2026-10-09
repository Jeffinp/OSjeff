use super::*;

#[test]
fn breadcrumbs_of_paths() {
    let _g = crate::i18n::testlang::LangGuard::new(crate::i18n::Lang::Pt);
    let c = breadcrumbs(b"/");
    assert_eq!(c.len(), 1);
    assert_eq!(c[0].label, b"Disco");
    let c = breadcrumbs(b"/a/b c/d");
    let labels: Vec<_> = c.iter().map(|x| x.label.clone()).collect();
    assert_eq!(
        labels,
        vec![
            b"Disco".to_vec(),
            b"a".to_vec(),
            b"b c".to_vec(),
            b"d".to_vec()
        ]
    );
    let paths: Vec<_> = c.iter().map(|x| x.path.clone()).collect();
    assert_eq!(
        paths,
        vec![
            b"/".to_vec(),
            b"/a".to_vec(),
            b"/a/b c".to_vec(),
            b"/a/b c/d".to_vec()
        ]
    );
}

#[test]
fn breadcrumbs_of_the_trash() {
    let _g = crate::i18n::testlang::LangGuard::new(crate::i18n::Lang::Pt);
    let c = breadcrumbs(TRASH_PATH);
    assert_eq!(c.len(), 2);
    assert_eq!(c[1].label, b"Lixeira");
    assert_eq!(c[1].path, TRASH_PATH);
}

#[test]
fn history_back_and_forward() {
    let mut h = History::new(b"/");
    assert!(!h.can_back() && !h.can_forward());
    h.push(b"/a");
    h.push(b"/a/b");
    assert_eq!(h.current(), b"/a/b");
    assert_eq!(h.back(), Some(&b"/a"[..]));
    assert_eq!(h.back(), Some(&b"/"[..]));
    assert_eq!(h.back(), None);
    assert_eq!(h.forward(), Some(&b"/a"[..]));
    h.push(b"/c");
    assert!(!h.can_forward());
    assert_eq!(h.back(), Some(&b"/a"[..]));
}

#[test]
fn history_ignores_a_repeat_and_is_bounded() {
    let mut h = History::new(b"/");
    h.push(b"/");
    assert!(!h.can_back());
    for i in 0..200 {
        let p = alloc::format!("/d{i}");
        h.push(p.as_bytes());
    }
    let mut steps = 0;
    while h.back().is_some() {
        steps += 1;
    }
    assert!(steps <= HISTORY_MAX);
}
