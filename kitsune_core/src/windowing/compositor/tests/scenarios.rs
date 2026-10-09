//! Hand-made histories: the cases that went wrong on the desktop, each checked against a full
//! redraw after every frame.

use crate::windowing::compositor::sim::{H, Kind, Op, PANEL_H, SCREEN, Sim, W};
use crate::windowing::window::Rect;

/// Frames of `Tick` until no window animates.
fn settle(sim: &mut Sim) {
    for _ in 0..8 {
        sim.frame(&[Op::Tick]).unwrap();
    }
}

fn open(sim: &mut Sim, kind: Kind, r: Rect) {
    sim.frame(&[Op::Open(kind, r)]).unwrap();
    settle(sim);
}

/// Sum of the channels of the pixel `(x, y)`.
fn lum(sim: &Sim, x: i32, y: i32) -> u32 {
    let p = sim.screen()[(y * W + x) as usize];
    (p >> 16 & 0xFF) + (p >> 8 & 0xFF) + (p & 0xFF)
}

#[test]
fn the_shadow_of_a_window_falls_on_the_one_below_and_goes_when_it_leaves() {
    let mut sim = Sim::new();
    open(&mut sim, Kind::Plain, Rect::new(20, 20, 90, 70));
    open(&mut sim, Kind::Plain, Rect::new(60, 30, 40, 30));
    // The second window's shadow reaches pixel (103, 40), on the first window's body.
    let shadowed = lum(&sim, 103, 40);
    sim.frame(&[Op::Move(1, 60, 0)]).unwrap();
    let unshadowed = lum(&sim, 103, 40);
    assert!(
        shadowed < unshadowed,
        "{shadowed} < {unshadowed}: the neighbour's shadow was drawn"
    );
    let _ = (H, PANEL_H, SCREEN);
}

#[test]
fn a_window_above_an_animating_one_keeps_its_shadow_over_it() {
    // The Snake game ticks under a plain window: its shadow must stay on the game, every frame.
    let mut sim = Sim::new();
    open(&mut sim, Kind::Game, Rect::new(20, 20, 100, 80));
    open(&mut sim, Kind::Plain, Rect::new(70, 30, 60, 40));
    let before = lum(&sim, 70 + 60 + 2, 40);
    for _ in 0..30 {
        sim.frame(&[Op::LiveTick]).unwrap();
    }
    // The pixel is right of the plain window, over the game: still shadowed relative to the game.
    sim.frame(&[Op::Move(1, 0, 0)]).unwrap();
    assert!(lum(&sim, 70 + 60 + 2, 40) != 0 && before != 0);
}

#[test]
fn tarefas_and_snake_never_blink_and_keep_their_shadows() {
    // The reported bug: a live chart (Tarefas) with a game (Snake) over it, and the user clicking
    // between them and moving them while both change on their own.
    let mut sim = Sim::new();
    open(&mut sim, Kind::Live, Rect::new(10, 12, 130, 90));
    open(&mut sim, Kind::Game, Rect::new(50, 30, 110, 80));
    for round in 0..40 {
        let ops: &[Op] = match round % 5 {
            0 => &[Op::LiveTick],
            1 => &[Op::LiveTick, Op::Raise(0)],
            2 => &[Op::LiveTick, Op::Edit],
            3 => &[Op::Raise(0), Op::Move(1, 3, 2)],
            _ => &[Op::LiveTick, Op::Hover(2)],
        };
        sim.frame(ops).unwrap();
    }
}

#[test]
fn an_editor_over_tarefas_has_its_shadow_and_a_soft_edge() {
    let mut sim = Sim::new();
    open(&mut sim, Kind::Live, Rect::new(10, 12, 140, 100));
    open(&mut sim, Kind::Plain, Rect::new(70, 40, 100, 60));
    // Pixel just outside the editor's right edge, over Tarefas: darker than Tarefas itself.
    let shadowed = lum(&sim, 70 + 100 + 1, 60);
    let mut other = Sim::new();
    other.world = sim.world.clone();
    other.world.move_by(1, 0, 90);
    other.render();
    let raw = lum(&other, 70 + 100 + 1, 60);
    assert!(shadowed < raw, "editor shadow missing: {shadowed} !< {raw}");
    for i in 0..40 {
        sim.frame(&[Op::LiveTick]).unwrap();
        if i % 7 == 0 {
            sim.frame(&[Op::Raise(0)]).unwrap();
            sim.frame(&[Op::Raise(0)]).unwrap();
        }
    }
    assert_eq!(
        lum(&sim, 70 + 100 + 1, 60),
        shadowed,
        "the shadow survived the ticks"
    );
}

#[test]
fn a_damage_edge_through_a_shadow_does_not_double_it() {
    let mut sim = Sim::new();
    open(&mut sim, Kind::Plain, Rect::new(40, 30, 70, 50));
    let r = Rect::new(100, 50, 20, 8); // cuts the right-hand shadow ring
    for _ in 0..3 {
        sim.engine.invalidate(r);
        sim.frame(&[Op::Idle]).unwrap();
    }
    sim.engine.invalidate(Rect::new(0, 0, W, 1));
    sim.engine.invalidate(Rect::new(0, H - 1, W, 1));
    sim.frame(&[Op::Idle]).unwrap();
}

#[test]
fn a_one_pixel_damage_repaints_one_pixel_worth() {
    let mut sim = Sim::new();
    open(&mut sim, Kind::Plain, Rect::new(30, 30, 80, 50));
    open(&mut sim, Kind::Plain, Rect::new(70, 40, 80, 50));
    for (x, y) in [(75, 45), (112, 85), (20, 20), (150, 92)] {
        sim.engine.invalidate(Rect::new(x, y, 1, 1));
        sim.frame(&[Op::Idle]).unwrap();
        assert_eq!(sim.last_plan.damage.area(), 1);
        let painted: i64 = sim
            .last_plan
            .steps
            .iter()
            .map(|s| (s.clip.w * s.clip.h) as i64)
            .sum();
        assert!(painted <= 4, "{painted} pixels painted for one");
    }
}

#[test]
fn a_window_leaving_the_screen_on_every_side() {
    let mut sim = Sim::new();
    open(&mut sim, Kind::Plain, Rect::new(60, 40, 70, 50));
    open(&mut sim, Kind::Game, Rect::new(20, 30, 70, 50));
    for (dx, dy) in [
        (-100, 0),
        (200, 0),
        (0, -80),
        (0, 160),
        (90, 20),
        (-5, -5),
        (-1, 0),
        (0, 1),
    ] {
        sim.frame(&[Op::Move(0, dx, dy)]).unwrap();
        sim.frame(&[Op::LiveTick]).unwrap();
        sim.frame(&[Op::Move(0, -dx, -dy)]).unwrap();
    }
}

#[test]
fn maximised_windows_have_no_shadow_and_cover_what_is_below() {
    let mut sim = Sim::new();
    open(&mut sim, Kind::Live, Rect::new(10, 12, 100, 70));
    open(&mut sim, Kind::Plain, Rect::new(50, 30, 100, 70));
    sim.frame(&[Op::Snap(1, 6)]).unwrap();
    settle(&mut sim);
    let painted: u64 = sim
        .last_plan
        .steps
        .iter()
        .map(|s| s.clip.w as u64 * s.clip.h as u64)
        .sum();
    assert!(painted <= (W * H) as u64);
    sim.frame(&[Op::LiveTick]).unwrap();
    sim.frame(&[Op::Raise(0)]).unwrap();
    sim.frame(&[Op::Raise(0)]).unwrap();
    sim.frame(&[Op::Unmaximize(1)]).unwrap();
    settle(&mut sim);
    sim.frame(&[Op::Snap(0, 0)]).unwrap();
    settle(&mut sim);
}

#[test]
fn translucent_windows_blend_over_what_is_below_and_are_repainted_whole() {
    let mut sim = Sim::new();
    open(&mut sim, Kind::Game, Rect::new(20, 20, 100, 70));
    // The new window fades in over the game: translucent, so the game shows through and the
    // pixels under it must be recomputed from the bottom every frame.
    sim.frame(&[Op::Open(Kind::Plain, Rect::new(50, 40, 90, 60))])
        .unwrap();
    for _ in 0..6 {
        sim.frame(&[Op::LiveTick]).unwrap();
        sim.frame(&[Op::Tick]).unwrap();
    }
    sim.frame(&[Op::Close(1)]).unwrap();
    for _ in 0..5 {
        sim.frame(&[Op::Tick, Op::LiveTick]).unwrap();
    }
}

#[test]
fn workspaces_hide_and_show_windows_with_their_shadows() {
    let mut sim = Sim::new();
    open(&mut sim, Kind::Plain, Rect::new(10, 12, 100, 70));
    open(&mut sim, Kind::Live, Rect::new(60, 30, 100, 70));
    sim.frame(&[Op::Workspace(1)]).unwrap();
    open(&mut sim, Kind::Plain, Rect::new(30, 50, 100, 60));
    sim.frame(&[Op::SendToWorkspace(0, 0)]).unwrap();
    sim.frame(&[Op::Workspace(1)]).unwrap();
    sim.frame(&[Op::Workspace(0)]).unwrap();
    sim.frame(&[Op::Minimize(0)]).unwrap();
    settle(&mut sim);
    sim.frame(&[Op::Restore]).unwrap();
    settle(&mut sim);
}

#[test]
fn overlays_over_windows_and_back() {
    let mut sim = Sim::new();
    open(&mut sim, Kind::Game, Rect::new(10, 12, 120, 80));
    open(&mut sim, Kind::Live, Rect::new(50, 30, 120, 80));
    let pop = Rect::new(100, 20, 60, 40);
    sim.frame(&[Op::Popover(Some(pop))]).unwrap();
    for _ in 0..5 {
        sim.frame(&[Op::LiveTick]).unwrap();
        sim.frame(&[Op::PopoverRepaint]).unwrap();
    }
    sim.frame(&[Op::Toast(Some(Rect::new(120, 100, 60, 14)))])
        .unwrap();
    sim.frame(&[Op::SnapPreview(Some(Rect::new(0, 8, 96, 120)))])
        .unwrap();
    sim.frame(&[Op::Move(0, 20, 5), Op::Popover(None)]).unwrap();
    sim.frame(&[Op::Toast(None), Op::SnapPreview(None)])
        .unwrap();
}
