use super::*;

const SAMPLE: &str =
    "# header\n\n  b.key = Second value  \r\na.key=First=with equals # not a comment\n\tc = \n";
const N: usize = count(SAMPLE);
static T: [(&str, &str); N] = build::<N>(SAMPLE);

#[test]
fn parses_sorts_and_trims() {
    assert_eq!(N, 3);
    let c = Catalog::new(&T);
    assert_eq!(c.len(), 3);
    assert_eq!(c.get("a.key"), Some("First=with equals # not a comment"));
    assert_eq!(c.get("b.key"), Some("Second value"));
    assert_eq!(c.get("c"), Some(""));
    assert_eq!(c.get("d"), None);
    assert_eq!(c.get(""), None);
    let keys: std::vec::Vec<_> = c.entries().iter().map(|e| e.0).collect();
    assert_eq!(keys, ["a.key", "b.key", "c"]);
}

#[test]
fn empty_source_is_an_empty_catalog() {
    const E: &str = "# nothing\n\n";
    const EN: usize = count(E);
    static ET: [(&str, &str); EN] = build::<EN>(E);
    assert_eq!(EN, 0);
    let c = Catalog::new(&ET);
    assert!(c.is_empty());
    assert_eq!(c.get("x"), None);
}

#[test]
fn utf8_values_survive() {
    const U: &str = "k = Configurações – “ok” …\nz = ç\n";
    const UN: usize = count(U);
    static UT: [(&str, &str); UN] = build::<UN>(U);
    let c = Catalog::new(&UT);
    assert_eq!(c.get("k"), Some("Configurações – “ok” …"));
    assert_eq!(c.get("z"), Some("ç"));
}

#[test]
fn last_line_without_newline_and_crlf() {
    const S: &str = "a = 1\r\nb = 2";
    const SN: usize = count(S);
    static ST: [(&str, &str); SN] = build::<SN>(S);
    let c = Catalog::new(&ST);
    assert_eq!((c.get("a"), c.get("b")), (Some("1"), Some("2")));
}

#[test]
fn sort_is_byte_order_and_lookup_finds_every_key() {
    const S: &str = "b = 1\nB = 2\na.b = 3\na = 4\na_ = 5\na.a = 6\nz = 7\n";
    const SN: usize = count(S);
    static ST: [(&str, &str); SN] = build::<SN>(S);
    let c = Catalog::new(&ST);
    for w in c.entries().windows(2) {
        assert!(w[0].0 < w[1].0);
    }
    for (k, v) in c.entries() {
        assert_eq!(c.get(k), Some(*v));
    }
}

#[test]
fn shell_sort_sorts_a_large_reversed_input() {
    // Built at run time here (the const path is the same code) over > 701 entries so
    // every gap of the sequence is exercised.
    let mut src = std::string::String::new();
    for i in (0..900).rev() {
        src.push_str(&std::format!("k.{i:04} = v{i}\n"));
    }
    let src: &'static str = std::boxed::Box::leak(src.into_boxed_str());
    let t = build::<900>(src);
    for (i, e) in t.iter().enumerate() {
        assert_eq!(e.0, std::format!("k.{i:04}"));
    }
}
