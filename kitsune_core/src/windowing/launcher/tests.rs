use super::*;

const ITEMS: [(&str, Category); 6] = [
    ("Terminal", Category::System),
    ("Editor", Category::Utilities),
    ("Navegador", Category::Internet),
    ("Imagens", Category::Media),
    ("Calculadora", Category::Utilities),
    ("Notas", Category::Utilities),
];

#[test]
fn the_rail_has_five_stable_entries() {
    let labels: Vec<&str> = CATEGORIES.iter().map(|c| c.label()).collect();
    assert_eq!(
        labels,
        ["Todos", "Sistema", "Internet", "Mídia", "Utilitários"]
    );
    for (i, c) in CATEGORIES.iter().enumerate() {
        assert_eq!(c.index(), i);
    }
}

#[test]
fn the_built_in_tables_cover_every_app() {
    for n in ["shell", "files", "taskmgr", "monitor", "settings", "syslog"] {
        assert_eq!(system_category(n), Category::System, "{n}");
    }
    assert_eq!(system_category("browser"), Category::Internet);
    assert_eq!(system_category("viewer"), Category::Media);
    for n in ["editor", "calc"] {
        assert_eq!(system_category(n), Category::Utilities, "{n}");
    }
    assert_eq!(app_category("paint"), Category::Media);
    assert_eq!(app_category("snake"), Category::Media);
    // Unknown ids land in Utilitários; no app is ever in All.
    assert_eq!(app_category("anything-else"), Category::Utilities);
    assert_ne!(system_category("zzz"), Category::All);
}

#[test]
fn a_category_keeps_the_original_order_and_all_shows_everything() {
    let all = filter(ITEMS.iter().copied(), Category::All, "");
    assert_eq!(all, [0, 1, 2, 3, 4, 5]);
    assert_eq!(
        filter(ITEMS.iter().copied(), Category::Utilities, ""),
        [1, 4, 5]
    );
    assert_eq!(filter(ITEMS.iter().copied(), Category::Media, ""), [3]);
    assert_eq!(
        filter(ITEMS.iter().copied(), Category::Internet, "   "),
        [2]
    );
}

#[test]
fn a_search_looks_everywhere_and_ranks_prefixes_first() {
    // "n" is a prefix of Navegador and Notas, inside Terminal's word? no: only contained.
    let hits = filter(ITEMS.iter().copied(), Category::Media, "n");
    assert_eq!(hits[0], 2); // Navegador (prefix), whatever the category
    assert!(hits.contains(&5) && hits.contains(&0));
    // Accents and case do not matter.
    assert_eq!(filter(ITEMS.iter().copied(), Category::All, "CALC"), [4]);
    assert!(filter(ITEMS.iter().copied(), Category::All, "zzz").is_empty());
    // Ties keep the original order.
    let tie = filter(ITEMS.iter().copied(), Category::All, "e");
    let mut sorted = tie.clone();
    sorted.sort_by_key(|&i| (search::rank("e", ITEMS[i].0), i));
    assert_eq!(tie, sorted);
}

#[test]
fn recents_are_newest_first_unique_and_bounded() {
    let mut r = Recents::new();
    assert!(r.list().is_empty());
    for k in ["a", "b", "c", "a", "d", "e", "f", "g"] {
        r.note(k);
    }
    assert_eq!(r.list().len(), RECENTS);
    // "a" was launched again before d, e, f, g, so it is the oldest that is left; b and c fell off.
    assert_eq!(r.list(), ["g", "f", "e", "d", "a"]);
    r.note("e");
    assert_eq!(r.list()[0], "e");
    assert_eq!(r.list().iter().filter(|k| *k == "e").count(), 1);
    r.forget("e");
    assert!(!r.list().iter().any(|k| k == "e"));
    r.forget("never-launched");
}
