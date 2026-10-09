use super::*;

#[test]
fn rotate90_known_values() {
    // 1 2 3        4 1
    // 4 5 6   ->   5 2
    //              6 3
    let i = img(3, 2, &[1, 2, 3, 4, 5, 6]);
    let r = i.rotate90().unwrap();
    assert_eq!((r.width(), r.height()), (2, 3));
    assert_eq!(r.pixels(), &[4, 1, 5, 2, 6, 3]);
}

#[test]
fn rotate270_known_values() {
    let i = img(3, 2, &[1, 2, 3, 4, 5, 6]);
    let r = i.rotate270().unwrap();
    assert_eq!((r.width(), r.height()), (2, 3));
    assert_eq!(r.pixels(), &[3, 6, 2, 5, 1, 4]);
}

#[test]
fn four_quarter_turns_are_the_identity() {
    let i = pattern(7, 4, true);
    let mut r = i.clone();
    for _ in 0..4 {
        r = r.rotate90().unwrap();
    }
    assert_eq!(r, i);
    let mut l = i.clone();
    for _ in 0..4 {
        l = l.rotate270().unwrap();
    }
    assert_eq!(l, i);
    assert_eq!(i.rotate90().unwrap().rotate270().unwrap(), i);
}

#[test]
fn rotate180_equals_two_quarter_turns_and_two_flips() {
    let i = pattern(5, 3, false);
    let mut a = i.clone();
    a.rotate180();
    assert_eq!(a, i.rotate90().unwrap().rotate90().unwrap());
    let mut b = i.clone();
    b.flip_horizontal();
    b.flip_vertical();
    assert_eq!(a, b);
    a.rotate180();
    assert_eq!(a, i);
}

#[test]
fn flips_are_involutions_with_known_values() {
    let i = img(3, 2, &[1, 2, 3, 4, 5, 6]);
    let mut h = i.clone();
    h.flip_horizontal();
    assert_eq!(h.pixels(), &[3, 2, 1, 6, 5, 4]);
    let mut v = i.clone();
    v.flip_vertical();
    assert_eq!(v.pixels(), &[4, 5, 6, 1, 2, 3]);
    h.flip_horizontal();
    v.flip_vertical();
    assert_eq!(h, i);
    assert_eq!(v, i);
}

#[test]
fn flip_vertical_handles_odd_and_single_rows() {
    let mut i = img(2, 3, &[1, 2, 3, 4, 5, 6]);
    i.flip_vertical();
    assert_eq!(i.pixels(), &[5, 6, 3, 4, 1, 2]);
    let mut one = img(3, 1, &[1, 2, 3]);
    one.flip_vertical();
    assert_eq!(one.pixels(), &[1, 2, 3]);
    let mut col = img(1, 4, &[1, 2, 3, 4]);
    col.flip_vertical();
    assert_eq!(col.pixels(), &[4, 3, 2, 1]);
    col.flip_horizontal();
    assert_eq!(col.pixels(), &[4, 3, 2, 1]);
}

#[test]
fn rotating_a_single_pixel_or_line() {
    let p = img(1, 1, &[42]);
    assert_eq!(p.rotate90().unwrap(), p);
    assert_eq!(p.rotate270().unwrap(), p);
    let row = img(4, 1, &[1, 2, 3, 4]);
    let r = row.rotate90().unwrap();
    assert_eq!(
        (r.width(), r.height(), r.pixels()),
        (1, 4, &[1, 2, 3, 4][..])
    );
    let l = row.rotate270().unwrap();
    assert_eq!(l.pixels(), &[4, 3, 2, 1]);
}
