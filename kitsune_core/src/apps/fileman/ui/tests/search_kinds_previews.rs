use super::*;

#[test]
fn search_ignores_case_and_accents() {
    assert!(matches_query("Ação.txt".as_bytes(), b"acao"));
    assert!(matches_query("Ação.txt".as_bytes(), "AÇÃO".as_bytes()));
    assert!(matches_query(b"relatorio-final.pdf", b"FINAL"));
    assert!(!matches_query(b"relatorio.pdf", b"final"));
    assert!(matches_query(b"x", b""));
    assert!(matches_query(b"x", b"   "));
    assert!(matches_query(b"my file", b" my f "));
    assert_eq!(search_key("Ação".as_bytes()), "acao");
}

#[test]
fn kinds_follow_the_extension() {
    assert_eq!(preview_kind(b"a", true), PreviewKind::Folder);
    assert_eq!(preview_kind(b"a.PNG", false), PreviewKind::Image);
    assert_eq!(preview_kind(b"a.wasm", false), PreviewKind::App);
    assert_eq!(preview_kind(b"notas.txt", false), PreviewKind::Text);
    assert_eq!(preview_kind(b"a.bin", false), PreviewKind::Other);
    use crate::ui::appart::FileKind;
    assert_eq!(icon_kind(b"d", true), FileKind::Folder);
    assert_eq!(icon_kind(b"a.bmp", false), FileKind::Image);
    assert_eq!(icon_kind(b"a.bin", false), FileKind::Generic);
    let _g = crate::i18n::testlang::LangGuard::new(crate::i18n::Lang::Pt);
    assert_eq!(kind_label(b"d", true), "Pasta");
    assert_eq!(kind_label(b"a.png", false), "Imagem PNG");
    assert_eq!(kind_label(b"a.txt", false), "Texto TXT");
    assert_eq!(kind_label(b"LEIAME", false), "Texto");
    assert_eq!(kind_label(b"a.bin", false), "Arquivo BIN");
    assert_eq!(kind_label(b"a.wasm", false), "Aplicativo");
    drop(_g);
    let _g = crate::i18n::testlang::LangGuard::new(crate::i18n::Lang::En);
    assert_eq!(kind_label(b"d", true), "Folder");
    assert_eq!(kind_label(b"a.png", false), "PNG image");
    assert_eq!(kind_label(b"a.txt", false), "TXT text");
    assert_eq!(kind_label(b"LEIAME", false), "Text");
    assert_eq!(kind_label(b"a.bin", false), "BIN file");
    assert_eq!(kind_label(b"a.wasm", false), "Application");
}

#[test]
fn text_previews_are_clean_and_bounded() {
    let t = text_preview(b"um\ndois\tcom tab\r\ntres\n\n\n", 10, 40);
    assert_eq!(t, vec!["um", "dois    com tab", "tres"]);
    let long = text_preview("x".repeat(500).as_bytes(), 3, 20);
    assert_eq!(long[0].len(), 20);
    let many: Vec<u8> = (0..100)
        .flat_map(|i| alloc::format!("l{i}\n").into_bytes())
        .collect();
    assert_eq!(text_preview(&many, 7, 40).len(), 7);
    assert_eq!(text_preview("ação ✓\n".as_bytes(), 3, 40), vec!["ação ✓"]);
    // Binary data gives nothing to show.
    assert!(text_preview(&[0, 1, 2, 3, 255, 0, 9], 5, 20).is_empty());
    assert!(text_preview(b"", 5, 20).is_empty());
    // Control characters become spaces.
    assert_eq!(
        text_preview(b"abcdefghijklmnopqrstuvwxyz\x07abc", 2, 40),
        vec!["abcdefghijklmnopqrstuvwxyz abc"]
    );
}

#[test]
fn modified_dates_say_today_and_yesterday() {
    use crate::i18n::{Lang, testlang::LangGuard};
    let _g = LangGuard::new(Lang::Pt);
    // 2026-10-08 14:32:00 UTC; local time is UTC-3.
    let now = 1_791_469_920u64;
    let tz = -3 * 3600;
    assert_eq!(format_modified(0, now, tz, true), "--");
    assert_eq!(format_modified(now - 600, now, tz, true), "Hoje, 11:22");
    assert_eq!(format_modified(now - 86_400, now, tz, true), "Ontem, 11:32");
    assert_eq!(
        format_modified(now - 5 * 86_400, now, tz, true),
        crate::apps::fileman::format_datetime(now - 5 * 86_400, tz, true)
    );
    // Just after local midnight is still "today"; the minute before is "yesterday".
    let midnight_local = now - (now as i64 + tz as i64).rem_euclid(86_400) as u64;
    assert!(format_modified(midnight_local, now, tz, true).starts_with("Hoje"));
    assert!(format_modified(midnight_local - 1, now, tz, true).starts_with("Ontem"));
    // A file from the future (a clock that moved back) is not "yesterday".
    assert!(!format_modified(now + 10 * 86_400, now, tz, true).starts_with("Ontem"));
}

#[test]
fn modified_dates_in_english_use_the_chosen_clock() {
    use crate::i18n::{Lang, testlang::LangGuard};
    let _g = LangGuard::new(Lang::En);
    let now = 1_791_469_920u64;
    let tz = -3 * 3600;
    assert_eq!(
        format_modified(now - 600, now, tz, false),
        "Today, 11:22 AM"
    );
    assert_eq!(format_modified(now - 600, now, tz, true), "Today, 11:22");
    assert_eq!(
        format_modified(now - 86_400, now, tz, false),
        "Yesterday, 11:32 AM"
    );
    assert_eq!(
        format_modified(now - 5 * 86_400, now, tz, false),
        "10/03/2026 11:32 AM"
    );
    assert_eq!(format_modified(0, now, tz, false), "--");
}
