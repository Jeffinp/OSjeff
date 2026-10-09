use super::*;

#[test]
fn unfilter_none_sub_up() {
    let mut r = [1u8, 2, 3, 4];
    unfilter(0, &mut r, &[9; 4], 1).unwrap();
    assert_eq!(r, [1, 2, 3, 4]);
    let mut r = [1u8, 1, 1, 1];
    unfilter(1, &mut r, &[0; 4], 1).unwrap();
    assert_eq!(r, [1, 2, 3, 4]);
    let mut r = [1u8, 1, 1, 1, 1, 1];
    unfilter(1, &mut r, &[0; 6], 3).unwrap();
    assert_eq!(r, [1, 1, 1, 2, 2, 2]);
    let mut r = [1u8, 2, 3];
    unfilter(2, &mut r, &[10, 20, 30], 1).unwrap();
    assert_eq!(r, [11, 22, 33]);
    // Wrapping arithmetic.
    let mut r = [250u8, 10];
    unfilter(2, &mut r, &[10, 250], 1).unwrap();
    assert_eq!(r, [4, 4]);
}

#[test]
fn unfilter_average_values_by_hand() {
    // x0 = 10 + (0 + 20)/2 = 20 ; x1 = 10 + (20 + 20)/2 = 30 ; x2 = 10 + (30 + 20)/2 = 35
    let mut r = [10u8, 10, 10];
    unfilter(3, &mut r, &[20, 20, 20], 1).unwrap();
    assert_eq!(r, [20, 30, 35]);
    // Sum of left and up above 255 must not overflow: (255 + 255) / 2 = 255: x1 = 0 + (127 + 255) / 2 = 191.
    let mut r = [0u8, 0];
    unfilter(3, &mut r, &[255, 255], 1).unwrap();
    assert_eq!(r, [127, 191]);
}

#[test]
fn paeth_predictor_known_cases() {
    assert_eq!(paeth(0, 0, 0), 0);
    assert_eq!(paeth(10, 20, 10), 20); // p = 20: closest to b
    assert_eq!(paeth(20, 10, 10), 20); // p = 20: closest to a
    assert_eq!(paeth(10, 10, 20), 10); // p = 0: a and b tie (10), a wins
    assert_eq!(paeth(100, 50, 75), 75); // p = 75 exactly c
    assert_eq!(paeth(255, 0, 255), 0);
    assert_eq!(paeth(0, 255, 0), 255);
    assert_eq!(paeth(7, 7, 7), 7);
}

#[test]
fn unfilter_paeth_first_row_and_with_history() {
    // First row (prev all zero) degenerates to Sub.
    let mut r = [1u8, 1, 1, 1];
    unfilter(4, &mut r, &[0; 4], 1).unwrap();
    assert_eq!(r, [1, 2, 3, 4]);
    // With a previous row, bpp 2.
    let mut r = [5u8, 5, 5, 5, 5, 5];
    let prev = [10u8, 20, 30, 40, 50, 60];
    unfilter(4, &mut r, &prev, 2).unwrap();
    assert_eq!(r[0], 15);
    assert_eq!(r[1], 25);
    let p2 = paeth(r[0], prev[2], prev[0]);
    assert_eq!(r[2], 5u8.wrapping_add(p2));
}

#[test]
fn unfilter_rejects_unknown_types_and_handles_tiny_rows() {
    let mut r = [0u8; 3];
    assert_eq!(unfilter(5, &mut r, &[0; 3], 1), Err(PngError::BadFilter));
    // A row shorter than bpp (e.g. a 1-pixel RGBA8 row has len 4, bpp 4).
    for ft in 0..5u8 {
        let mut r = [1u8, 2, 3, 4];
        unfilter(ft, &mut r, &[10, 20, 30, 40], 4).unwrap();
    }
    let mut r = [9u8];
    unfilter(1, &mut r, &[0], 4).unwrap();
    assert_eq!(r, [9]);
    unfilter(3, &mut r, &[2], 4).unwrap();
    unfilter(4, &mut r, &[2], 4).unwrap();
    let mut empty: [u8; 0] = [];
    unfilter(4, &mut empty, &[], 1).unwrap();
}
