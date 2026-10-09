use super::*;
use alloc::string::ToString;
use alloc::vec;

const M: Metrics = Metrics { cw: 9, lh: 22 };

#[test]
fn the_grid_fits_inside_the_window() {
    for (w, h) in [(600, 360), (320, 180), (1000, 700), (100, 60)] {
        let r = Rect::new(40, 50, w, h);
        let g = layout(r, M);
        assert!(g.cols >= 1 && g.rows >= 1);
        if w >= 200 && h >= 140 {
            assert!(g.x + g.cols as i32 * M.cw <= r.right() - BAR_W + 1);
            assert!(g.y + g.rows as i32 * M.lh <= r.bottom());
        }
        assert_eq!(g.strip.y, r.y + TITLE_H);
        assert!(g.strip.bottom() <= g.y);
    }
}

#[test]
fn a_press_maps_to_its_cell_and_clamps() {
    let g = layout(Rect::new(0, 0, 600, 360), M);
    assert_eq!(g.cell_at(g.x, g.y), (0, 0));
    assert_eq!(g.cell_at(g.x + 9 * 3 + 4, g.y + 22 * 2 + 5), (2, 3));
    assert_eq!(g.cell_at(-50, -50), (0, 0));
    assert_eq!(g.cell_at(5000, 5000), (g.rows - 1, g.cols - 1));
    assert!(g.in_text(g.x, g.y));
    assert!(!g.in_text(g.x, g.strip.y));
    let r = g.cell_rect(1, 2);
    assert_eq!(g.cell_at(r.x + 1, r.y + 1), (1, 2));
    assert!(g.track().right() == g.window.right());
}

#[test]
fn selections_order_and_span() {
    let mut s = Selection::new((2, 5));
    assert!(s.is_empty());
    assert_eq!(s.span(2, 10), None);
    s.head = (1, 3);
    assert_eq!(s.ordered(), ((1, 3), (2, 5)));
    // The first row runs from the start cell to the end of the row.
    assert_eq!(s.span(1, 10), Some((3, 8)));
    assert_eq!(s.span(2, 10), Some((0, 6)));
    assert_eq!(s.span(0, 10), None);
    assert_eq!(s.span(3, 10), None);
    // One row: just the cells between.
    let one = Selection {
        anchor: (4, 2),
        head: (4, 6),
    };
    assert_eq!(one.span(4, 20), Some((2, 5)));
}

#[test]
fn words_are_runs_of_one_kind() {
    assert_eq!(word_bounds("ls -la /home/ana", 0), (0, 1));
    assert_eq!(word_bounds("ls -la /home/ana", 4), (3, 5));
    assert_eq!(word_bounds("ls -la /home/ana", 10), (7, 15));
    assert_eq!(word_bounds("ls  x", 2), (2, 3));
    assert_eq!(word_bounds("a;b", 1), (1, 1));
    assert_eq!(word_bounds("", 3), (0, 0));
    // Past the end lands on the last cell.
    assert_eq!(word_bounds("abc", 40), (0, 2));
}

#[test]
fn extracted_text_cuts_blanks_and_joins_rows() {
    let rows = vec![
        "/ $ ls   ".to_string(),
        "apps  etc  home   ".to_string(),
        "$".to_string(),
    ];
    let s = Selection {
        anchor: (0, 4),
        head: (1, 8),
    };
    assert_eq!(extract(&rows, &s), "ls\napps  etc");
    let whole = Selection {
        anchor: (0, 0),
        head: (2, 0),
    };
    assert_eq!(extract(&rows, &whole), "/ $ ls\napps  etc  home\n$");
    assert_eq!(extract(&rows, &Selection::new((0, 0))), "");
    // A selection that runs past the rows does not panic.
    let far = Selection {
        anchor: (1, 0),
        head: (9, 3),
    };
    assert_eq!(extract(&rows, &far), "apps  etc  home\n$");
}

#[test]
fn the_prompt_splits_into_path_and_symbol() {
    assert_eq!(split_prompt("/home $ "), ("/home ", "$ "));
    assert_eq!(split_prompt("~/docs# "), ("~/docs", "# "));
    assert_eq!(split_prompt("> "), ("", "> "));
    assert_eq!(split_prompt("sem simbolo"), ("sem simbolo", ""));
    assert_eq!(tab_label("/ $ "), "/");
    assert_eq!(tab_label("$ "), "~");
    assert_eq!(tab_label("/Documentos $ "), "/Documentos");
}
