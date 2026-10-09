use super::*;

#[test]
fn panel_items_flow_from_both_edges_and_the_clock_is_centred() {
    let g = panel_layout(1280, &[60, 16], 110, &[pill_width(3)]);
    assert_eq!(
        g.left[0],
        Rect::new(PANEL_EDGE, 0, 60 + 2 * PANEL_PAD, PANEL_H)
    );
    assert_eq!(g.left[1].x, g.left[0].right() + PANEL_GAP);
    // The clock sits on the screen's centre line, whatever the sides hold.
    let cx = g.center.x + g.center.w / 2;
    assert!((cx - 640).abs() <= 1);
    assert_eq!(g.right[0].right(), 1280 - PANEL_EDGE);
    assert!(g.left[1].right() < g.center.x && g.center.right() < g.right[0].x);
    for r in g.left.iter().chain(&g.right).chain([&g.center]) {
        assert_eq!((r.y, r.h), (0, PANEL_H));
    }
    assert_eq!(rect_at(&g.left, g.left[1].x + 3, 10), Some(1));
    assert_eq!(rect_at(&g.left, 600, 10), None);
    assert_eq!(rect_at(&g.right, g.right[0].x, PANEL_H - 1), Some(0));
    assert_eq!(rect_at(&g.right, g.right[0].x, PANEL_H), None);
    // Several right items keep their order with the gap between them.
    let two = panel_layout(1280, &[], 100, &[20, 30]);
    assert_eq!(two.right[1].right(), 1280 - PANEL_EDGE);
    assert_eq!(two.right[0].right() + PANEL_GAP, two.right[1].x);
}

#[test]
fn the_workspace_dots_have_a_stable_width_and_a_longer_current_one() {
    for n in 2..=4u8 {
        let g = panel_layout(1280, &[60, workspace_width(n)], 110, &[pill_width(3)]);
        let item = g.left[1];
        assert_eq!(item.w, workspace_width(n) + 2 * PANEL_PAD);
        for cur in 0..n {
            let dots: Vec<Rect> = (0..n).map(|i| workspace_dot(item, i, cur)).collect();
            assert_eq!(dots[cur as usize].w, WS_CUR_W);
            assert!(
                dots.iter()
                    .enumerate()
                    .all(|(i, d)| i == cur as usize || d.w == WS_DOT)
            );
            // Side by side with the gap, inside the item, centred vertically.
            for w in dots.windows(2) {
                assert_eq!(w[1].x - w[0].right(), WS_GAP);
            }
            assert_eq!(dots[0].x, item.x + PANEL_PAD);
            assert_eq!(dots[n as usize - 1].right() + PANEL_PAD, item.right());
            assert!(
                dots.iter()
                    .all(|d| d.y - item.y == item.bottom() - d.bottom())
            );
            // Hit testing: each dot's own pixels and the half gaps, the whole bar height.
            for (i, d) in dots.iter().enumerate() {
                assert_eq!(workspace_at(item, n, cur, d.x + 1, 2), Some(i as u8));
                assert_eq!(
                    workspace_at(item, n, cur, d.right() - 1, PANEL_H - 1),
                    Some(i as u8)
                );
            }
        }
    }
    let item = Rect::new(100, 0, workspace_width(3) + 2 * PANEL_PAD, PANEL_H);
    assert_eq!(workspace_at(item, 3, 0, item.x + 1, 5), None); // the padding is not a dot
    assert_eq!(workspace_at(item, 3, 0, 5, 5), None);
    assert_eq!(workspace_width(0), 0);
}

#[test]
fn the_status_pill_places_its_icons_on_a_pitch() {
    let g = panel_layout(1280, &[], 100, &[pill_width(3)]);
    let pill = g.right[0];
    assert_eq!(pill.w, pill_width(3) + 2 * PANEL_PAD);
    let icons: Vec<Rect> = (0..3).map(|i| pill_icon(pill, i)).collect();
    assert_eq!(icons[1].x - icons[0].x, PILL_PITCH);
    assert_eq!(icons[0].x, pill.x + PANEL_PAD);
    assert_eq!(icons[2].right() + PANEL_PAD, pill.right());
    for i in &icons {
        assert_eq!((i.w, i.h), (PILL_ICON, PILL_ICON));
        assert_eq!(i.y - pill.y, pill.bottom() - i.bottom());
    }
}

#[test]
fn menu_geometry_sizes_to_content_and_stays_on_screen() {
    let rows = [
        MenuRow::Item {
            label_w: 90,
            shortcut_w: 30,
        },
        MenuRow::Separator,
        MenuRow::Item {
            label_w: 200,
            shortcut_w: 0,
        },
    ];
    let g = menu_geom(&rows, (10, 28), 1280, 720);
    assert_eq!(g.rows.len(), 3);
    assert_eq!(g.rect.h, 2 * MENU_PAD_Y + 2 * MENU_ROW_H + MENU_SEP_H);
    assert!(g.rect.w >= MENU_MIN_W);
    assert!(g.rect.w >= 2 * MENU_PAD_X + MENU_CHECK_W + 200 + 12);
    assert_eq!(g.rows[0].y, g.rect.y + MENU_PAD_Y);
    assert_eq!(g.rows[1].y, g.rows[0].bottom());
    assert_eq!(g.rows[2].y, g.rows[1].bottom());
    assert!(
        g.rows
            .iter()
            .all(|r| r.x >= g.rect.x && r.right() <= g.rect.right())
    );
    // Near the corner it shifts back inside; never under the menu bar.
    let c = menu_geom(&rows, (1270, 715), 1280, 720);
    assert!(c.rect.right() <= 1276 && c.rect.bottom() <= 716);
    let top = menu_geom(&rows, (100, 0), 1280, 720);
    assert_eq!(top.rect.y, MENUBAR_H);
    // Hit testing skips the separator.
    assert_eq!(
        menu_row_at(&g, &rows, g.rows[0].x + 4, g.rows[0].y + 4),
        Some(0)
    );
    assert_eq!(
        menu_row_at(&g, &rows, g.rows[1].x + 4, g.rows[1].y + 4),
        None
    );
    assert_eq!(
        menu_row_at(&g, &rows, g.rows[2].x + 4, g.rows[2].y + 4),
        Some(2)
    );
    assert_eq!(menu_row_at(&g, &rows, 0, 0), None);
}

#[test]
fn the_launcher_has_a_rail_a_field_recents_and_a_scrolling_grid() {
    let g = launcher_grid(1280, 720, 21, 0, 3);
    // The rail is a column of five rows at the left; the content starts right of it.
    assert_eq!(g.rail_rows.len(), 5);
    assert_eq!(g.rail_rows[1].y, g.rail_rows[0].bottom());
    assert!(
        g.rail_rows
            .iter()
            .all(|r| r.x >= g.rail.x && r.right() <= g.rail.right())
    );
    assert!(g.rail_rows[4].bottom() <= g.rail.bottom());
    assert!(g.left > g.rail.right());
    // Field on top of the content, recents under it, grid under them.
    assert_eq!(g.field.x, g.left);
    assert!(g.field.bottom() < g.recents_label.y);
    assert_eq!(g.recents.len(), 3);
    assert_eq!(g.recents[1].x, g.recents[0].right() + 8);
    assert!(g.recents[0].bottom() < g.top);
    assert_eq!(g.cells.len(), 21);
    assert!(g.cols >= 6 && g.total_rows == 21usize.div_ceil(g.cols));
    // Row-major and aligned.
    assert_eq!(g.cells[1].x, g.cells[0].right());
    assert_eq!(g.cells[g.cols].y, g.cells[0].bottom());
    assert_eq!(g.cells[g.cols].x, g.cells[0].x);
    assert_eq!(g.cells[0].x, g.left);
    // Everything fits on the screen's right side.
    assert!(g.cells[g.cols - 1].right() <= 1280 - 40);
    // Hit tests: cells inside the visible rows, rail rows, recents; nothing elsewhere.
    let c = g.cells[3];
    assert_eq!(launcher_cell_at(&g, 720, c.x + 4, c.y + 4), Some(3));
    assert_eq!(launcher_cell_at(&g, 720, 5, 5), None);
    let rr = g.rail_rows[2];
    assert_eq!(launcher_rail_at(&g, rr.x + 3, rr.y + 3), Some(2));
    assert_eq!(launcher_rail_at(&g, g.cells[0].x, g.cells[0].y), None);
    let rc = g.recents[2];
    assert_eq!(launcher_recent_at(&g, rc.x + 3, rc.y + 3), Some(2));
    // No recents: the row disappears and the grid moves up.
    let none = launcher_grid(1280, 720, 21, 0, 0);
    assert!(none.recents.is_empty() && none.top < g.top);
    // Never more than the remembered recents.
    assert_eq!(
        launcher_grid(1280, 720, 4, 0, 9).recents.len(),
        crate::windowing::launcher::RECENTS
    );
    // Scrolling moves rows up (clamped).
    let s = launcher_grid(1280, 300, 40, 99, 0);
    assert!(s.visible_rows >= 1);
    assert!(s.cells[0].y < s.top);
    // A narrow screen still has columns.
    assert!(launcher_grid(600, 720, 5, 0, 0).cols >= 2);
}

#[test]
fn spotlight_panel_grows_with_results() {
    let a = spotlight_geom(1280, 720, 0);
    let b = spotlight_geom(1280, 720, 5);
    assert_eq!(a.panel.h, SPOT_FIELD_H);
    assert_eq!(b.panel.h, SPOT_FIELD_H + 5 * SPOT_ROW_H + 16);
    assert_eq!(b.rows.len(), 5);
    assert_eq!(a.panel.x, b.panel.x);
    assert_eq!(a.panel.y, b.panel.y);
    assert!(
        b.rows
            .iter()
            .all(|r| b.panel.contains(r.x, r.y) && r.bottom() <= b.panel.bottom())
    );
    // Capped.
    assert_eq!(spotlight_geom(1280, 720, 99).rows.len(), SPOT_MAX_ROWS);
    assert!(a.panel.y >= MENUBAR_H);
}

#[test]
fn popovers_and_toasts_sit_under_the_bar() {
    let anchor = Rect::new(1100, 0, 40, 28);
    let p = popover_rect(anchor, 320, 300, 1280);
    assert_eq!(p.y, MENUBAR_H + 6);
    assert_eq!(p.right(), 1140);
    let edge = popover_rect(Rect::new(1260, 0, 40, 28), 320, 300, 1280);
    assert_eq!(edge.right(), 1272);
    let left = popover_rect(Rect::new(0, 0, 20, 28), 320, 300, 1280);
    assert_eq!(left.x, 8);
    let t0 = toast_rect(0, 1280);
    let t1 = toast_rect(1, 1280);
    assert_eq!(t0.right(), 1280 - TOAST_MARGIN);
    assert!(t0.y >= MENUBAR_H);
    assert_eq!(t1.y, t0.bottom() + TOAST_GAP);
}

#[test]
fn popover_contents_fit_their_popovers() {
    let r = Rect::new(900, 36, QUICK_W, QUICK_H);
    let g = quick_geom(r);
    let inside = |x: &Rect, r: &Rect| {
        x.x >= r.x && x.right() <= r.right() && x.y >= r.y && x.bottom() <= r.bottom()
    };
    for x in [g.title, g.accent_label, g.restart, g.shutdown]
        .iter()
        .chain(g.tiles.iter())
        .chain(g.swatches.iter())
    {
        assert!(inside(x, &r), "{x:?}");
    }
    // Tiles: two columns, three rows, no overlap, in reading order.
    assert_eq!(g.tiles[1].x, g.tiles[0].right() + 8);
    assert_eq!(g.tiles[2].y, g.tiles[0].bottom() + 8);
    assert_eq!(g.tiles[1].right(), r.right() - 16);
    assert!(g.tiles[QUICK_TILES - 1].bottom() <= g.accent_label.y);
    assert!(g.swatches[0].bottom() <= g.restart.y);
    for w in g.swatches.windows(2) {
        assert!(w[0].right() <= w[1].x);
    }
    assert!(g.restart.right() < g.shutdown.x);
    assert_eq!(
        quick_tile_at(&g, g.tiles[3].x + 4, g.tiles[3].y + 4),
        Some(QuickTile::DoNotDisturb)
    );
    assert_eq!(quick_tile_at(&g, r.x, r.y), None);

    let cr = Rect::new(300, 36, CENTRE_W, CENTRE_H);
    let c = centre_geom(cr);
    for x in [
        c.day,
        c.date,
        c.notif_title,
        c.clear,
        c.empty,
        c.dnd_label,
        c.dnd_switch,
        c.calendar,
    ]
    .iter()
    .chain(c.rows.iter())
    {
        assert!(inside(x, &cr), "{x:?}");
    }
    // Notifications at the left, the calendar at the right, the rows stacked.
    assert!(c.empty.right() < c.calendar.x);
    assert!(c.rows[CENTRE_ROWS - 1].bottom() <= c.dnd_label.y);
    assert!(c.notif_title.right() <= c.clear.x);
    assert!(c.dnd_label.right() <= c.dnd_switch.x);
    for w in c.rows.windows(2) {
        assert!(w[0].bottom() <= w[1].y);
    }
    let cal = calendar_geom(c.calendar);
    assert!(cal.cells[5][6].bottom() <= cr.bottom() && cal.cells[5][6].right() <= cr.right());
    assert!(cal.prev.right() <= cal.next.x && cal.next.right() <= c.calendar.right());
    assert!(cal.title.right() <= cal.prev.x);
    assert_eq!(cal.cells[0][1].x - cal.cells[0][0].x, cal.cells[0][0].w);
    // The centred popover stays under the panel and on the screen.
    let p = popover_centered(Rect::new(560, 0, 160, PANEL_H), CENTRE_W, CENTRE_H, 1280);
    assert_eq!((p.y, p.w), (PANEL_H + 6, CENTRE_W));
    assert!(((p.x + p.w / 2) - 640).abs() <= 1);
    assert_eq!(
        popover_centered(Rect::new(0, 0, 40, PANEL_H), CENTRE_W, CENTRE_H, 1280).x,
        8
    );
}

#[test]
fn calendar_weekdays_and_grid() {
    assert_eq!(weekday(1970, 1, 1), 4);
    assert_eq!(weekday(2000, 1, 1), 6); // Saturday
    assert_eq!(weekday(2024, 2, 29), 4); // Thursday
    assert_eq!(weekday(2026, 10, 8), 4); // Thursday
    let g = month_grid(2026, 10);
    assert_eq!(g[0][4], 1); // Oct 1 2026 is a Thursday
    let days: Vec<u8> = g.iter().flatten().copied().filter(|&d| d != 0).collect();
    assert_eq!(days.len(), 31);
    assert_eq!(days, (1..=31).collect::<Vec<u8>>());
    // February in a leap and a common year.
    let n = |y| {
        month_grid(y, 2)
            .iter()
            .flatten()
            .filter(|&&d| d != 0)
            .count()
    };
    assert_eq!((n(2024), n(2025)), (29, 28));
    // Never needs a seventh row.
    for y in 2020..2030 {
        for m in 1..=12 {
            assert_eq!(
                month_grid(y, m)
                    .iter()
                    .flatten()
                    .filter(|&&d| d != 0)
                    .count(),
                crate::format::unixtime::days_in_month(y, m) as usize
            );
        }
    }
}
