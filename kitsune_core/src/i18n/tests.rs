use super::*;

/// Tests that change the global language take this lock (the test harness is parallel).
pub(crate) static LANG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn lookup_in_each_language_and_fallback_chain() {
    assert_eq!(tr_in(Lang::Pt, "meta.name"), "Português (Brasil)");
    assert_eq!(tr_in(Lang::En, "meta.name"), "English");
    // Unknown keys come back as themselves, in any language.
    assert_eq!(tr_in(Lang::Pt, "no.such.key"), "no.such.key");
    assert_eq!(tr_in(Lang::En, ""), "");
}

#[test]
fn missing_lookups_are_counted() {
    let before = missing_count();
    let _ = tr_in(Lang::Pt, "still.no.such.key");
    assert!(missing_count() > before);
    let b2 = missing_count();
    let _ = tr_in(Lang::Pt, "meta.code");
    assert_eq!(missing_count(), b2);
}

#[test]
fn current_language_switches_and_bumps_the_generation() {
    let _g = LANG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    set_lang(Lang::Pt);
    let g0 = generation();
    assert_eq!(tr("meta.name"), "Português (Brasil)");
    set_lang(Lang::Pt);
    assert_eq!(generation(), g0, "same language: no new generation");
    set_lang(Lang::En);
    assert_eq!(generation(), g0 + 1);
    assert_eq!(tr("meta.name"), "English");
    assert_eq!(lang(), Lang::En);
    set_lang(Lang::Pt);
}

#[test]
fn language_tags() {
    assert_eq!(Lang::Pt.code(), "pt-BR");
    assert_eq!(Lang::En.code(), "en");
    for (tag, l) in [
        ("pt", Some(Lang::Pt)),
        ("pt-BR", Some(Lang::Pt)),
        ("PT_br", Some(Lang::Pt)),
        ("pt-PT", Some(Lang::Pt)),
        ("en", Some(Lang::En)),
        ("en-US", Some(Lang::En)),
        ("EN_gb", Some(Lang::En)),
        ("fr", None),
        ("", None),
        ("p", None),
        ("português", None),
        ("pt-BR-extra-way-too-long", None),
    ] {
        assert_eq!(Lang::from_code(tag.as_bytes()), l, "{tag}");
    }
    for l in Lang::ALL {
        assert_eq!(Lang::from_code(l.code().as_bytes()), Some(l));
        assert_eq!(Lang::from_index(l.index()), l);
    }
    assert_eq!(Lang::from_index(200), Lang::DEFAULT);
    assert_eq!(Lang::Pt.native_name(), "Português (Brasil)");
    assert_eq!(Lang::En.native_name(), "English");
}

catalog::catalog_src!(
    FIX_PT,
    "fix.items.one = {n} item\nfix.items.other = {n} itens\nfix.only_other.other = so {n}\n"
);
catalog::catalog_src!(
    FIX_EN,
    "fix.items.one = {n} item\nfix.items.other = {n} items\nfix.en_only.other = en {n}\n"
);

#[test]
fn plural_keys_use_each_languages_rule() {
    assert_eq!(Lang::Pt.plural_rule(), plural::Rule::ZeroAndOneSingular);
    assert_eq!(Lang::En.plural_rule(), plural::Rule::OneIsSingular);
    let pt = [&FIX_PT, &FIX_EN];
    let en = [&FIX_EN, &FIX_PT];
    let zo = plural::Rule::ZeroAndOneSingular;
    let one = plural::Rule::OneIsSingular;
    assert_eq!(plural_chain(&pt, zo, "fix.items", 0), Some("{n} item"));
    assert_eq!(plural_chain(&pt, zo, "fix.items", 1), Some("{n} item"));
    assert_eq!(plural_chain(&pt, zo, "fix.items", 2), Some("{n} itens"));
    assert_eq!(plural_chain(&en, one, "fix.items", 0), Some("{n} items"));
    assert_eq!(plural_chain(&en, one, "fix.items", 1), Some("{n} item"));
    // `.one` missing: `.other` stands in; missing in the language: the next catalog.
    assert_eq!(plural_chain(&pt, zo, "fix.only_other", 1), Some("so {n}"));
    assert_eq!(plural_chain(&pt, zo, "fix.en_only", 5), Some("en {n}"));
    assert_eq!(plural_chain(&pt, zo, "nope", 1), None);
    let long = "k".repeat(500);
    assert_eq!(plural_chain(&pt, zo, &long, 1), None);
    assert_eq!(plural_in(Lang::En, &long, 1), long);
    assert_eq!(plural_in(Lang::En, "nope", 1), "nope");
}

#[test]
fn plural_fmt_binds_n() {
    let chain = [&FIX_EN];
    let t = plural_chain(&chain, plural::Rule::OneIsSingular, "fix.items", 1234).unwrap();
    let mut o = String::new();
    render(&mut o, Lang::En, t, &[("n", Arg::Num(1234))]).unwrap();
    assert_eq!(o, "1,234 items");
    // Through the public API: a key that is in no catalog formats as itself.
    assert_eq!(
        plural_fmt_in(Lang::En, "no.such.family", 2, &[]),
        "no.such.family"
    );
    assert_eq!(
        plural_fmt_in(Lang::Pt, "x.y {n}", 3, &[]),
        "x.y {n}".replace("{n}", "3")
    );
}

#[test]
fn formatted_lookup() {
    let a: Args = &[("n", Arg::Int(3)), ("size", bytes(1536))];
    assert_eq!(
        tr_fmt_in(
            Lang::Pt,
            "fmt.time.24",
            &[("hour", Arg::Pad(7, 2)), ("minute", Arg::Pad(5, 2))]
        ),
        "07:05"
    );
    // A missing key formats the key itself: placeholders in it still work.
    assert_eq!(tr_fmt_in(Lang::En, "x {n} {size}", a), "x 3 1.5 KiB");
    assert_eq!(tr_fmt_in(Lang::Pt, "x {n} {size}", a), "x 3 1,5 KiB");
}

#[test]
fn macros() {
    let _g = LANG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    set_lang(Lang::En);
    assert_eq!(crate::t!("meta.name"), "English");
    assert_eq!(crate::t!("fmt.am"), "AM");
    assert_eq!(crate::t!("no.key.here", n = 7), "no.key.here");
    assert_eq!(crate::tp!("no.family", 3u32), "no.family");
    assert_eq!(crate::tk!("a.b"), "a.b");
    set_lang(Lang::Pt);
    assert_eq!(crate::t!("meta.name"), "Português (Brasil)");
}

#[test]
fn date_helpers_follow_the_current_language() {
    let _g = LANG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let c = Civil {
        year: 2026,
        month: 10,
        day: 8,
        weekday: 4,
        hour: 23,
        minute: 49,
        second: 0,
    };
    set_lang(Lang::Pt);
    assert_eq!(
        format_date(c, DateStyle::Full, true),
        "qui, 8 out 2026 23:49"
    );
    assert_eq!(format_time(c, true, false), "23:49");
    assert_eq!(format_size(1536), "1,5 KiB");
    assert_eq!(format_num(1234567), "1.234.567");
    set_lang(Lang::En);
    assert_eq!(
        format_date(c, DateStyle::Full, false),
        "Thu, Oct 8 2026 11:49 PM"
    );
    assert_eq!(format_time(c, false, true), "11:49:00 PM");
    assert_eq!(format_size(1536), "1.5 KiB");
    assert_eq!(format_num(1234567), "1,234,567");
    set_lang(Lang::Pt);
}
