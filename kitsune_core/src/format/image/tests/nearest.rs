use super::*;

#[test]
fn nearest_upscale_replicates_pixels() {
    let i = img(2, 2, &[RED, GREEN, BLUE, WHITE]);
    let o = i.resize_nearest(4, 4).unwrap();
    for y in 0..4 {
        for x in 0..4 {
            assert_eq!(o.get(x, y), i.get(x / 2, y / 2), "({x},{y})");
        }
    }
}

#[test]
fn nearest_downscale_samples_centres() {
    let i = pattern(8, 8, false);
    let o = i.resize_nearest(4, 4).unwrap();
    for y in 0..4 {
        for x in 0..4 {
            // Centre of destination pixel x maps to source x*2+1 (2x2 block, round up).
            assert_eq!(o.get(x, y), i.get(x * 2 + 1, y * 2 + 1));
        }
    }
}

#[test]
fn nearest_same_size_is_identity_and_keeps_alpha() {
    let i = pattern(7, 5, true);
    assert_eq!(i.resize_nearest(7, 5).unwrap(), i);
    let o = i.resize_nearest(13, 9).unwrap();
    assert_eq!(o.width(), 13);
    assert!(o.pixels().iter().all(|p| i.pixels().contains(p)));
}

#[test]
fn nearest_to_one_pixel_and_odd_ratios() {
    let i = pattern(5, 3, false);
    let o = i.resize_nearest(1, 1).unwrap();
    assert_eq!(o.get(0, 0), i.get(2, 1));
    let o = i.resize_nearest(7, 2).unwrap();
    assert_eq!((o.width(), o.height()), (7, 2));
}
