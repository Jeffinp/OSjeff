use super::*;

#[test]
fn stats_bit_balance_on_1_mib() {
    let out = mib();
    let n = (out.len() * 8) as f64;
    let ones: u64 = out.iter().map(|b| u64::from(b.count_ones())).sum();
    // Binomial(n, 1/2): sigma = sqrt(n)/2. Allow 5 sigma.
    let dev = (ones as f64 - n / 2.0).abs();
    assert!(dev < 5.0 * n.sqrt() / 2.0, "ones {ones} of {n}");
}

#[test]
fn stats_byte_chi_square_on_1_mib() {
    let out = mib();
    let mut hist = [0u64; 256];
    for &b in &out {
        hist[usize::from(b)] += 1;
    }
    let exp = out.len() as f64 / 256.0;
    let chi: f64 = hist
        .iter()
        .map(|&o| {
            let d = o as f64 - exp;
            d * d / exp
        })
        .sum();
    // df = 255: mean 255, sigma sqrt(510) = 22.6; bounds are about +-5 sigma.
    assert!((140.0..370.0).contains(&chi), "chi-square {chi}");
}

#[test]
fn stats_adjacent_bit_transitions_on_1_mib() {
    let out = mib();
    // Count bit flips between consecutive bits (LSB first): about half.
    let mut flips = 0u64;
    let mut prev = out[0] & 1;
    let mut total = 0u64;
    for &b in &out {
        for i in 0..8 {
            let bit = (b >> i) & 1;
            flips += u64::from(bit != prev);
            prev = bit;
            total += 1;
        }
    }
    let dev = (flips as f64 - total as f64 / 2.0).abs();
    assert!(
        dev < 5.0 * (total as f64).sqrt() / 2.0,
        "flips {flips}/{total}"
    );
}

#[test]
fn stats_every_bit_position_is_balanced() {
    let out = mib();
    for pos in 0..8 {
        let ones = out.iter().filter(|b| (**b >> pos) & 1 == 1).count() as f64;
        let n = out.len() as f64;
        assert!(
            (ones - n / 2.0).abs() < 5.0 * n.sqrt() / 2.0,
            "bit {pos}: {ones}"
        );
    }
}
