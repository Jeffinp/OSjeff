use crate::compositor::sim::{Source, SplitMix};
use crate::compositor::{MAX_RECTS, Region, area, subtract};
use crate::window::Rect;

const G: i32 = 40;

/// Every invariant of a region.
fn check(r: &Region) {
    assert!(r.len() <= MAX_RECTS, "too many rects: {:?}", r);
    for (i, a) in r.rects().iter().enumerate() {
        assert!(!a.is_empty(), "empty rect in {:?}", r);
        for b in &r.rects()[i + 1..] {
            assert!(a.intersection(b).is_none(), "overlap {a:?} {b:?} in {r:?}");
        }
    }
}

fn pixels(r: &Region) -> Vec<bool> {
    let mut v = vec![false; (G * G) as usize];
    for y in 0..G {
        for x in 0..G {
            v[(y * G + x) as usize] = r.contains_point(x, y);
        }
    }
    v
}

fn rect(s: &mut SplitMix) -> Rect {
    let w = s.between(0, 18);
    let h = s.between(0, 18);
    Rect::new(s.between(-4, G - 6), s.between(-4, G - 6), w, h)
}

#[test]
fn empty_and_single() {
    let mut r = Region::new();
    assert!(r.is_empty());
    r.add(Rect::new(1, 1, 0, 5));
    assert!(r.is_empty(), "an empty rect adds nothing");
    r.add(Rect::new(2, 3, 4, 5));
    assert_eq!(r.rects(), &[Rect::new(2, 3, 4, 5)]);
    assert_eq!(r.area(), 20);
    assert_eq!(r.bounds(), Some(Rect::new(2, 3, 4, 5)));
}

#[test]
fn contained_rect_changes_nothing_and_swallowing_replaces() {
    let mut r = Region::from_rect(Rect::new(0, 0, 10, 10));
    r.add(Rect::new(2, 2, 3, 3));
    assert_eq!(r.rects().len(), 1);
    r.add(Rect::new(-1, -1, 20, 20));
    assert_eq!(r.rects(), &[Rect::new(-1, -1, 20, 20)]);
}

#[test]
fn subtract_leaves_exactly_the_rest() {
    let a = Rect::new(0, 0, 10, 10);
    let mut out = Vec::new();
    subtract(&a, &Rect::new(3, 3, 4, 4), &mut out);
    assert_eq!(out.iter().map(area).sum::<u64>(), 100 - 16);
    subtract(&a, &Rect::new(20, 20, 4, 4), &mut Vec::new());
    let mut out = Vec::new();
    subtract(&a, &Rect::new(-5, -5, 50, 50), &mut out);
    assert!(out.is_empty());
}

#[test]
fn covers_and_intersects() {
    let mut r = Region::new();
    r.add(Rect::new(0, 0, 10, 10));
    r.add(Rect::new(10, 0, 10, 10));
    assert!(r.covers(&Rect::new(5, 2, 10, 5)), "spans two rects");
    assert!(!r.covers(&Rect::new(5, 2, 10, 15)));
    assert!(r.intersects(&Rect::new(19, 9, 5, 5)));
    assert!(!r.intersects(&Rect::new(20, 0, 5, 5)));
}

#[test]
fn random_adds_are_supersets_and_stay_valid() {
    for seed in 0..400u64 {
        let mut s = SplitMix(seed);
        let mut r = Region::new();
        let mut want = vec![false; (G * G) as usize];
        for _ in 0..s.between(1, 30) {
            let q = rect(&mut s);
            r.add(q);
            check(&r);
            for y in 0..G {
                for x in 0..G {
                    if q.contains(x, y) {
                        want[(y * G + x) as usize] = true;
                    }
                }
            }
            let got = pixels(&r);
            for i in 0..want.len() {
                assert!(!want[i] || got[i], "seed {seed}: pixel {i} lost");
            }
        }
        // The merge may grow the region but never wildly: bounded by the bounding box.
        if let Some(b) = r.bounds() {
            assert!(r.area() <= area(&b));
        }
    }
}

#[test]
fn random_subtraction_is_exact() {
    for seed in 0..400u64 {
        let mut s = SplitMix(seed ^ 0xABCD);
        let mut r = Region::new();
        for _ in 0..s.between(1, 6) {
            r.add(rect(&mut s));
        }
        let before = pixels(&r);
        let cut = rect(&mut s);
        let mut after = r.clone();
        after.subtract_rect(&cut);
        check(&after);
        let got = pixels(&after);
        for y in 0..G {
            for x in 0..G {
                let i = (y * G + x) as usize;
                assert_eq!(
                    got[i],
                    before[i] && !cut.contains(x, y),
                    "seed {seed} ({x},{y})"
                );
            }
        }
    }
}

#[test]
fn clip_keeps_only_the_inside() {
    let mut r = Region::new();
    r.add(Rect::new(-5, -5, 20, 20));
    r.add(Rect::new(30, 30, 20, 20));
    let screen = Rect::new(0, 0, 40, 40);
    let c = r.clipped(&screen);
    check(&c);
    assert!(c.rects().iter().all(|q| screen.intersection(q) == Some(*q)));
    assert!(c.contains_point(0, 0) && c.contains_point(39, 39) && !c.contains_point(20, 20));
}

#[test]
fn two_far_apart_rects_stay_two() {
    let mut r = Region::new();
    r.add(Rect::new(0, 0, 10, 10));
    r.add(Rect::new(100, 100, 10, 10));
    assert_eq!(
        r.len(),
        2,
        "merging them would repaint 12 000 pixels for 200"
    );
}

#[test]
fn offset_rects_of_a_drag_merge_into_one() {
    let mut r = Region::new();
    r.add(Rect::new(100, 100, 600, 400));
    r.add(Rect::new(108, 106, 600, 400));
    check(&r);
    assert_eq!(r.len(), 1, "{:?}", r);
}

#[test]
fn many_small_rects_are_capped() {
    let mut r = Region::new();
    for i in 0..60 {
        r.add(Rect::new(i * 30, (i * 7) % 90, 4, 4));
        check(&r);
    }
    assert!(r.len() <= MAX_RECTS);
}
