use super::*;

#[test]
fn fit_dims_keeps_aspect_ratio() {
    let i = Image::new(1024, 768, 0).unwrap();
    assert_eq!(i.fit_dims(320, 240, false), Ok((320, 240)));
    assert_eq!(i.fit_dims(500, 100, false), Ok((133, 100)));
    assert_eq!(i.fit_dims(100, 500, false), Ok((100, 75)));
    assert_eq!(i.fit_dims(64, 64, false), Ok((64, 48)));
    let tall = Image::new(768, 1024, 0).unwrap();
    assert_eq!(tall.fit_dims(64, 64, false), Ok((48, 64)));
}

#[test]
fn fit_dims_only_upscales_on_request() {
    let i = Image::new(100, 50, 0).unwrap();
    assert_eq!(i.fit_dims(400, 400, false), Ok((100, 50)));
    assert_eq!(i.fit_dims(400, 400, true), Ok((400, 200)));
    assert_eq!(i.fit_dims(100, 50, false), Ok((100, 50)));
    // Too big in one dimension only: shrinks.
    assert_eq!(i.fit_dims(50, 400, false), Ok((50, 25)));
}

#[test]
fn fit_dims_extreme_aspect_ratios_stay_at_least_one() {
    let line = Image::new(1000, 1, 0).unwrap();
    assert_eq!(line.fit_dims(10, 10, false), Ok((10, 1)));
    let col = Image::new(1, 1000, 0).unwrap();
    assert_eq!(col.fit_dims(10, 10, false), Ok((1, 10)));
    assert_eq!(line.fit_dims(0, 10, false), Err(ImageError::ZeroSize));
    assert_eq!(line.fit_dims(10, 0, true), Err(ImageError::ZeroSize));
}

#[test]
fn fit_dims_never_exceeds_the_box() {
    for (sw, sh) in [
        (1, 1),
        (3, 7),
        (640, 480),
        (1920, 1080),
        (17, 1000),
        (999, 2),
    ] {
        let i = Image::new(sw, sh, 0).unwrap();
        for (bw, bh) in [(1, 1), (2, 5), (64, 64), (200, 100), (1000, 3), (7, 999)] {
            let (w, h) = i.fit_dims(bw, bh, true).unwrap();
            assert!(
                w >= 1 && h >= 1 && w <= bw && h <= bh,
                "{sw}x{sh} in {bw}x{bh} -> {w}x{h}"
            );
            // One of the two dimensions touches the box.
            assert!(w == bw || h == bh, "{sw}x{sh} in {bw}x{bh} -> {w}x{h}");
        }
    }
}

#[test]
fn fit_resamples_to_the_fitted_size() {
    let i = pattern(40, 30, false);
    let t = i.fit(16, 16, false, Filter::Auto).unwrap();
    assert_eq!((t.width(), t.height()), (16, 12));
    let same = i.fit(100, 100, false, Filter::Auto).unwrap();
    assert_eq!(same, i);
    let big = i.fit(80, 80, true, Filter::Bilinear).unwrap();
    assert_eq!((big.width(), big.height()), (80, 60));
}
