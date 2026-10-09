use super::*;
use alloc::string::String;

fn r(lang: Lang, t: &str, args: Args<'_>) -> String {
    let mut s = String::new();
    render(&mut s, lang, t, args).unwrap();
    s
}

#[test]
fn named_and_positional() {
    let a: Args = &[("n", Arg::Int(3)), ("who", Arg::Str("Ana"))];
    assert_eq!(r(Lang::En, "{who} has {n} files", a), "Ana has 3 files");
    assert_eq!(r(Lang::En, "{1} has {0} files", a), "Ana has 3 files");
    assert_eq!(r(Lang::Pt, "{who}{who}", a), "AnaAna");
}

#[test]
fn braces_escape() {
    assert_eq!(r(Lang::En, "{{x}} {{", &[]), "{x} {");
    assert_eq!(r(Lang::En, "a}b", &[]), "a}b");
    assert_eq!(r(Lang::En, "}}", &[]), "}");
    assert_eq!(r(Lang::En, "{{{n}}}", &[("n", Arg::Int(1))]), "{1}");
}

#[test]
fn malformed_is_verbatim() {
    let a: Args = &[("n", Arg::Int(3))];
    assert_eq!(r(Lang::En, "{missing}", a), "{missing}");
    assert_eq!(r(Lang::En, "{}", a), "{}");
    assert_eq!(r(Lang::En, "{n", a), "{n");
    assert_eq!(r(Lang::En, "{ {n}", a), "{ 3");
    assert_eq!(r(Lang::En, "{9}", a), "{9}");
    assert_eq!(
        r(Lang::En, "{99999999999999999999}", a),
        "{99999999999999999999}"
    );
    let long = std::format!("{{{}}}", "x".repeat(MAX_NAME + 1));
    assert_eq!(r(Lang::En, &long, a), long);
    assert_eq!(r(Lang::En, "ação {n} é {", a), "ação 3 é {");
}

#[test]
fn typed_arguments_per_language() {
    let a: Args = &[
        ("q", Arg::Num(1_234_567)),
        ("d", Arg::Dec(12_345, 1)),
        ("b", Arg::Bytes(1536)),
        ("p", Arg::Pad(7, 2)),
        ("i", Arg::Int(1234)),
        ("neg", Arg::Num(-1000)),
    ];
    assert_eq!(
        r(Lang::Pt, "{q}|{d}|{b}|{p}|{i}|{neg}", a),
        "1.234.567|1.234,5|1,5 KiB|07|1234|-1.000"
    );
    assert_eq!(
        r(Lang::En, "{q}|{d}|{b}|{p}|{i}|{neg}", a),
        "1,234,567|1,234.5|1.5 KiB|07|1234|-1,000"
    );
}

#[test]
fn capitalised_placeholder() {
    let a: Args = &[
        ("month", Arg::Str("março")),
        ("n", Arg::Int(1)),
        ("e", Arg::Str("")),
    ];
    assert_eq!(r(Lang::Pt, "{Month} de {month}", a), "Março de março");
    assert_eq!(
        r(Lang::Pt, "{Month}", &[("month", Arg::Str("épico"))]),
        "Épico"
    );
    // An argument that is itself capitalised wins; an empty one prints nothing.
    assert_eq!(r(Lang::Pt, "[{E}]", a), "[]");
    assert_eq!(r(Lang::Pt, "{N}", a), "1");
    assert_eq!(r(Lang::Pt, "{Nope}", a), "{Nope}");
    assert_eq!(r(Lang::Pt, "{Month}", &[("Month", Arg::Str("x"))]), "x");
}

#[test]
fn display_arg_and_rendered_wrapper() {
    let d = 42u8;
    let a: Args = &[("x", Arg::Display(&d))];
    assert_eq!(r(Lang::En, "<{x}>", a), "<42>");
    let s = std::format!(
        "{}",
        Rendered {
            lang: Lang::Pt,
            template: "{x}!",
            args: a
        }
    );
    assert_eq!(s, "42!");
}

#[test]
fn from_impls() {
    assert!(matches!(Arg::from(3u8), Arg::Int(3)));
    assert!(matches!(Arg::from(-3i32), Arg::Int(-3)));
    assert!(matches!(Arg::from(u64::MAX), Arg::Int(i64::MAX)));
    assert!(matches!(Arg::from("a"), Arg::Str("a")));
    let s = String::from("b");
    assert!(matches!(Arg::from(&s), Arg::Str("b")));
}

#[test]
fn placeholder_listing() {
    assert_eq!(placeholders("{a} {{b}} {c} {} {d"), ["a", "c"]);
    assert_eq!(placeholders("no braces"), std::vec::Vec::<&str>::new());
    assert_eq!(placeholders("{0}{1}{0}"), ["0", "1", "0"]);
}

#[test]
fn hostile_templates_terminate() {
    let junk = [
        "{{{{{{{{",
        "}}}}}}}",
        "{",
        "}",
        "{{}",
        "{}}",
        "{{{}}}{",
        "{ }",
        "{\u{1F600}}",
    ];
    let a: Args = &[("n", Arg::Int(1))];
    for t in junk {
        let _ = r(Lang::Pt, t, a);
    }
}
