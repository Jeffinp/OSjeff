//! The inverse DCT and the zigzag order.

/// Zigzag index -> position in the 8x8 block (row-major).
pub(super) const ZIGZAG: [u8; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20,
    13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59,
    52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

const fn fsh(x: i64) -> i64 {
    x * 4096
}

/// One 8-point pass in 12-bit fixed point (the constants are the usual cosine terms times 4096,
/// e.g. `2217 = 0.5411961 * 4096`). Returns `(x0, x1, x2, x3, t0, t1, t2, t3)`: the even
/// half and the odd half, to be combined as `x0 +/- t3`, `x1 +/- t2`, `x2 +/- t1`, `x3 +/- t0`.
#[inline]
#[allow(clippy::too_many_arguments)]
fn pass(s0: i64, s1: i64, s2: i64, s3: i64, s4: i64, s5: i64, s6: i64, s7: i64) -> [i64; 8] {
    let p1 = (s2 + s6) * 2217;
    let t2 = p1 + s6 * (-7567);
    let t3 = p1 + s2 * 3135;
    let t0 = fsh(s0 + s4);
    let t1 = fsh(s0 - s4);
    let x0 = t0 + t3;
    let x3 = t0 - t3;
    let x1 = t1 + t2;
    let x2 = t1 - t2;

    let (mut t0, mut t1, mut t2, mut t3) = (s7, s5, s3, s1);
    let mut p3 = t0 + t2;
    let mut p4 = t1 + t3;
    let mut p1 = t0 + t3;
    let mut p2 = t1 + t2;
    let p5 = (p3 + p4) * 4816;
    t0 *= 1223;
    t1 *= 8410;
    t2 *= 12586;
    t3 *= 6149;
    p1 = p5 + p1 * (-3685);
    p2 = p5 + p2 * (-10497);
    p3 *= -8034;
    p4 *= -1597;
    t3 += p1 + p4;
    t2 += p2 + p3;
    t1 += p2 + p4;
    t0 += p1 + p3;
    [x0, x1, x2, x3, t0, t1, t2, t3]
}

#[inline]
fn clamp8(v: i64) -> u8 {
    v.clamp(0, 255) as u8
}

/// Dequantised coefficients (row-major, DC first) -> 64 samples in 0..=255 (the +128 level shift
/// is included). The arithmetic is 64-bit so that coefficients from a hostile file (the caller
/// clamps them to 16 bits) cannot overflow in any build profile.
pub(super) fn idct(coef: &[i32; 64]) -> [u8; 64] {
    let mut tmp = [0i64; 64];
    // Columns.
    for c in 0..8 {
        let [mut x0, mut x1, mut x2, mut x3, t0, t1, t2, t3] = pass(
            coef[c] as i64,
            coef[8 + c] as i64,
            coef[16 + c] as i64,
            coef[24 + c] as i64,
            coef[32 + c] as i64,
            coef[40 + c] as i64,
            coef[48 + c] as i64,
            coef[56 + c] as i64,
        );
        // Bring the 12-bit scale back down but keep two extra bits of precision.
        x0 += 512;
        x1 += 512;
        x2 += 512;
        x3 += 512;
        tmp[c] = (x0 + t3) >> 10;
        tmp[56 + c] = (x0 - t3) >> 10;
        tmp[8 + c] = (x1 + t2) >> 10;
        tmp[48 + c] = (x1 - t2) >> 10;
        tmp[16 + c] = (x2 + t1) >> 10;
        tmp[40 + c] = (x2 - t1) >> 10;
        tmp[24 + c] = (x3 + t0) >> 10;
        tmp[32 + c] = (x3 - t0) >> 10;
    }
    // Rows.
    let mut out = [0u8; 64];
    for r in 0..8 {
        let v = &tmp[r * 8..r * 8 + 8];
        let [mut x0, mut x1, mut x2, mut x3, t0, t1, t2, t3] =
            pass(v[0], v[1], v[2], v[3], v[4], v[5], v[6], v[7]);
        // Final scale down, rounding, and the +128 level shift.
        let bias = 65536 + (128 << 17);
        x0 += bias;
        x1 += bias;
        x2 += bias;
        x3 += bias;
        let o = &mut out[r * 8..r * 8 + 8];
        o[0] = clamp8((x0 + t3) >> 17);
        o[7] = clamp8((x0 - t3) >> 17);
        o[1] = clamp8((x1 + t2) >> 17);
        o[6] = clamp8((x1 - t2) >> 17);
        o[2] = clamp8((x2 + t1) >> 17);
        o[5] = clamp8((x2 - t1) >> 17);
        o[3] = clamp8((x3 + t0) >> 17);
        o[4] = clamp8((x3 - t0) >> 17);
    }
    out
}
