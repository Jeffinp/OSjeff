//! The engine on hand-made scenes: what damages what, and the order and extent of the painting.

use crate::compositor::{Engine, Layer, LayerId, Plan, Scene};
use crate::window::Rect;

const SCREEN_W: i32 = 400;
const SCREEN_H: i32 = 300;

fn engine() -> Engine {
    Engine::new(SCREEN_W, SCREEN_H)
}

fn l(id: u32, r: Rect) -> Layer {
    Layer::new(LayerId(id), r)
}

fn scene(layers: &[Layer]) -> Scene {
    let mut s = Scene::new();
    for x in layers {
        s.push(*x);
    }
    s
}

fn painted_layers(p: &Plan) -> Vec<u32> {
    p.steps.iter().map(|s| s.layer.0).collect()
}

fn settled(e: &mut Engine, s: &Scene) {
    e.plan(s);
}

#[test]
fn the_first_plan_repaints_everything() {
    let mut e = engine();
    let p = e.plan(&scene(&[l(1, Rect::new(10, 10, 50, 50))]));
    assert_eq!(p.damage.area(), (SCREEN_W * SCREEN_H) as u64);
    assert_eq!(painted_layers(&p).first(), Some(&LayerId::WALLPAPER.0));
}

#[test]
fn an_unchanged_scene_plans_nothing() {
    let mut e = engine();
    let s = scene(&[
        l(1, Rect::new(10, 10, 50, 50)),
        l(2, Rect::new(30, 30, 60, 40)),
    ]);
    settled(&mut e, &s);
    let p = e.plan(&s);
    assert!(p.is_empty() && p.steps.is_empty());
}

#[test]
fn a_moved_layer_damages_where_it_was_and_where_it_is() {
    let mut e = engine();
    let a = Rect::new(10, 10, 50, 50);
    let b = Rect::new(200, 150, 50, 50);
    settled(&mut e, &scene(&[l(1, a)]));
    let p = e.plan(&scene(&[l(1, b)]));
    assert!(p.damage.covers(&a) && p.damage.covers(&b));
    assert_eq!(
        p.damage.len(),
        2,
        "far apart: two rectangles, not their bounding box"
    );
}

#[test]
fn a_one_pixel_move_damages_about_one_window() {
    let mut e = engine();
    let a = Rect::new(100, 100, 120, 80);
    settled(&mut e, &scene(&[l(1, a)]));
    let p = e.plan(&scene(&[l(1, Rect::new(101, 100, 120, 80))]));
    assert!(p.damage.covers(&a) && p.damage.covers(&Rect::new(101, 100, 120, 80)));
    assert!(p.damage.area() <= 122 * 80 + 10, "{}", p.damage.area());
}

#[test]
fn a_layer_that_appears_or_vanishes_damages_its_footprint() {
    let mut e = engine();
    let a = Rect::new(10, 10, 50, 50);
    let b = Rect::new(120, 30, 70, 40);
    settled(&mut e, &scene(&[l(1, a)]));
    let p = e.plan(&scene(&[l(1, a), l(2, b)]));
    assert!(p.damage.covers(&b) && p.damage.area() <= 70 * 40 + 10);
    let p = e.plan(&scene(&[l(1, a)]));
    assert!(
        p.damage.covers(&b),
        "the layer that left uncovers what was under it"
    );
}

#[test]
fn a_new_look_repaints_the_whole_footprint_and_dirty_only_its_rect() {
    let mut e = engine();
    let a = Rect::new(10, 10, 100, 100);
    settled(&mut e, &scene(&[l(1, a)]));
    let p = e.plan(&scene(&[l(1, a).with_look(1)]));
    assert!(p.damage.covers(&a));
    let chart = Rect::new(20, 80, 60, 20);
    let p = e.plan(&scene(&[l(1, a).with_look(1).with_dirty(Some(chart))]));
    assert!(p.damage.covers(&chart) && p.damage.area() <= 60 * 20 + 10);
    // A dirty rectangle outside the footprint is clipped to it.
    let p = e.plan(&scene(&[l(1, a)
        .with_look(1)
        .with_dirty(Some(Rect::new(300, 200, 40, 40)))]));
    assert!(p.is_empty());
}

#[test]
fn invalidate_adds_screen_damage_once() {
    let mut e = engine();
    let s = scene(&[l(1, Rect::new(10, 10, 50, 50))]);
    settled(&mut e, &s);
    e.invalidate(Rect::new(150, 5, 40, 12));
    let p = e.plan(&s);
    assert!(p.damage.covers(&Rect::new(150, 5, 40, 12)));
    assert!(e.plan(&s).is_empty(), "consumed by the plan");
    e.invalidate(Rect::new(-50, -50, 10, 10));
    assert!(e.plan(&s).is_empty(), "off screen: nothing");
    e.invalidate_all();
    assert_eq!(e.plan(&s).damage.area(), (SCREEN_W * SCREEN_H) as u64);
}

#[test]
fn a_swap_of_two_layers_damages_only_their_overlap() {
    let mut e = engine();
    let a = Rect::new(10, 10, 100, 100);
    let b = Rect::new(60, 60, 100, 100);
    settled(&mut e, &scene(&[l(1, a), l(2, b)]));
    let p = e.plan(&scene(&[l(2, b), l(1, a)]));
    let overlap = Rect::new(60, 60, 50, 50);
    assert!(p.damage.covers(&overlap));
    assert!(p.damage.area() <= 50 * 50 + 10, "{}", p.damage.area());
    // Disjoint layers that swap places change nothing.
    let c = Rect::new(250, 200, 50, 50);
    settled(&mut e, &scene(&[l(1, a), l(3, c)]));
    assert!(e.plan(&scene(&[l(3, c), l(1, a)])).is_empty());
}

#[test]
fn steps_run_bottom_to_top_and_stay_inside_footprints() {
    let mut e = engine();
    let a = Rect::new(10, 10, 100, 100);
    let b = Rect::new(60, 60, 100, 100);
    let s = scene(&[l(1, a), l(2, b)]);
    let p = e.plan(&s);
    assert_eq!(painted_layers(&p), vec![LayerId::WALLPAPER.0, 1, 2]);
    for st in &p.steps {
        if let Some(layer) = s.layers.iter().find(|x| x.id == st.layer) {
            assert_eq!(layer.footprint.intersection(&st.clip), Some(st.clip));
        }
    }
}

#[test]
fn an_opaque_layer_hides_what_is_under_it() {
    let mut e = engine();
    let big = Rect::new(0, 0, SCREEN_W, SCREEN_H);
    let s = scene(&[
        l(1, Rect::new(20, 20, 100, 100)),
        l(2, big).with_opaque(big),
    ]);
    let p = e.plan(&s);
    assert_eq!(
        painted_layers(&p),
        vec![2],
        "nothing below a full-screen opaque layer is painted"
    );
    // The reference plan paints everything.
    let full = e.full_plan(&s);
    assert_eq!(painted_layers(&full), vec![LayerId::WALLPAPER.0, 1, 2]);
    assert!(
        full.steps.iter().all(|st| st.clip == e.screen()),
        "the reference trusts no footprint"
    );
}

#[test]
fn translucent_layers_hide_nothing() {
    let mut e = engine();
    let big = Rect::new(0, 0, SCREEN_W, SCREEN_H);
    let s = scene(&[l(1, Rect::new(20, 20, 100, 100)), l(2, big)]);
    assert_eq!(
        painted_layers(&e.plan(&s)),
        vec![LayerId::WALLPAPER.0, 1, 2]
    );
}

#[test]
fn a_partly_covered_layer_is_painted_only_where_it_shows() {
    let mut e = engine();
    let a = Rect::new(0, 0, 200, 100);
    let top = Rect::new(0, 0, 100, 100);
    let s = scene(&[l(1, a), l(2, top).with_opaque(top)]);
    let p = e.plan(&s);
    let painted_a: u64 = p
        .steps
        .iter()
        .filter(|st| st.layer == LayerId(1))
        .map(|st| (st.clip.w * st.clip.h) as u64)
        .sum();
    assert_eq!(
        painted_a,
        100 * 100,
        "only the right half of the lower layer shows"
    );
}

#[test]
fn the_layer_footprint_is_clipped_to_the_screen() {
    let mut e = engine();
    let p = e.plan(&scene(&[l(1, Rect::new(-50, -50, 100, 100))]));
    assert!(p.steps.iter().all(|s| {
        s.clip.x >= 0 && s.clip.y >= 0 && s.clip.right() <= SCREEN_W && s.clip.bottom() <= SCREEN_H
    }));
}

#[test]
fn plan_damage_rects_are_disjoint_so_nothing_is_painted_twice() {
    let mut e = engine();
    let s0 = scene(&[
        l(1, Rect::new(10, 10, 100, 100)),
        l(2, Rect::new(80, 50, 100, 100)),
    ]);
    settled(&mut e, &s0);
    e.invalidate(Rect::new(20, 20, 200, 20));
    let p = e.plan(&scene(&[
        l(1, Rect::new(14, 12, 100, 100)),
        l(2, Rect::new(84, 55, 100, 100)).with_look(3),
    ]));
    let rs = p.damage.rects();
    for (i, a) in rs.iter().enumerate() {
        for b in &rs[i + 1..] {
            assert!(a.intersection(b).is_none());
        }
    }
    // Same for the clips of one layer.
    for layer in [LayerId::WALLPAPER, LayerId(1), LayerId(2)] {
        let clips: Vec<Rect> = p
            .steps
            .iter()
            .filter(|s| s.layer == layer)
            .map(|s| s.clip)
            .collect();
        for (i, a) in clips.iter().enumerate() {
            for b in &clips[i + 1..] {
                assert!(a.intersection(b).is_none(), "{layer:?}");
            }
        }
    }
}

#[test]
fn look_changes_with_every_input_and_with_their_order() {
    use crate::compositor::Look;
    let a = Look::new().u(1).u(2).get();
    assert_eq!(a, Look::new().u(1).u(2).get());
    assert_ne!(a, Look::new().u(2).u(1).get());
    assert_ne!(a, Look::new().u(1).u(3).get());
    assert_ne!(Look::new().b(true).get(), Look::new().b(false).get());
    assert_ne!(Look::new().i(-1).get(), Look::new().i(1).get());
    // No collisions over a small sweep of two inputs.
    let mut seen = std::collections::HashSet::new();
    for x in 0..200u64 {
        for y in 0..200u64 {
            assert!(seen.insert(Look::new().u(x).u(y).get()), "{x} {y}");
        }
    }
}
