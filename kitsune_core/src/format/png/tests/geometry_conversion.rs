use super::*;

#[test]
fn adam7_passes_cover_every_pixel_exactly_once() {
    for w in 1..=40usize {
        for h in 1..=40usize {
            let (ps, n) = passes(w, h, true);
            assert_eq!(n, 7);
            let mut seen = vec![0u8; w * h];
            for p in ps.iter().take(n) {
                for j in 0..p.h {
                    for i in 0..p.w {
                        seen[(p.y0 + j * p.dy) * w + p.x0 + i * p.dx] += 1;
                    }
                }
            }
            assert!(seen.iter().all(|&c| c == 1), "{w}x{h}");
        }
    }
}

#[test]
fn non_interlaced_is_a_single_pass_and_tiny_images_skip_passes() {
    let (ps, n) = passes(5, 3, false);
    assert_eq!((n, ps[0].w, ps[0].h, ps[0].dx), (1, 5, 3, 1));
    // 1x1: only pass 1 has pixels.
    let (ps, n) = passes(1, 1, true);
    let nonempty: Vec<bool> = ps.iter().take(n).map(|p| p.w > 0 && p.h > 0).collect();
    assert_eq!(nonempty, [true, false, false, false, false, false, false]);
}

#[test]
fn raw_size_formula() {
    let h = |w, hh, d, ct, il| Header {
        width: w,
        height: hh,
        bit_depth: d,
        color_type: parse_ihdr(&ihdr(w, hh, d, ct, il)).unwrap().color_type,
        interlaced: il == 1,
    };
    assert_eq!(raw_size(&h(4, 4, 8, 6, 0)), 4 * (1 + 16));
    assert_eq!(raw_size(&h(3, 2, 8, 2, 0)), 2 * (1 + 9));
    assert_eq!(raw_size(&h(9, 2, 1, 0, 0)), 2 * (1 + 2));
    assert_eq!(raw_size(&h(5, 5, 16, 6, 0)), 5 * (1 + 40));
    // Adam7 on 1x1 is a single 1-pixel row.
    assert_eq!(raw_size(&h(1, 1, 8, 6, 1)), 1 + 4);
    // Interlacing adds filter bytes, never removes pixels.
    assert!(raw_size(&h(8, 8, 8, 6, 1)) > raw_size(&h(8, 8, 8, 6, 0)));
}

#[test]
fn sixteen_to_eight_bit_rounding() {
    assert_eq!(s16(0), 0);
    assert_eq!(s16(65535), 255);
    assert_eq!(s16(128), 0);
    assert_eq!(s16(129), 1);
    assert_eq!(s16(257), 1);
    assert_eq!(s16(0x8080), 128);
    for v in 0..=255u32 {
        assert_eq!(s16((v * 257) as u16) as u32, v); // exact for replicated bytes
    }
}

#[test]
fn packed_samples() {
    assert_eq!(packed_sample(&[0b1010_0101], 0, 1), 1);
    assert_eq!(packed_sample(&[0b1010_0101], 1, 1), 0);
    assert_eq!(packed_sample(&[0b1010_0101], 7, 1), 1);
    assert_eq!(packed_sample(&[0b1110_0100], 0, 2), 3);
    assert_eq!(packed_sample(&[0b1110_0100], 1, 2), 2);
    assert_eq!(packed_sample(&[0b1110_0100], 3, 2), 0);
    assert_eq!(packed_sample(&[0xAB, 0xCD], 0, 4), 0xA);
    assert_eq!(packed_sample(&[0xAB, 0xCD], 1, 4), 0xB);
    assert_eq!(packed_sample(&[0xAB, 0xCD], 3, 4), 0xD);
    assert_eq!(packed_sample(&[], 5, 4), 0); // out of range reads as 0
}

#[test]
fn grey_levels_scale_to_full_range() {
    for (depth, max) in [(1u8, 1u8), (2, 3), (4, 15)] {
        // One pixel with the maximum sample value.
        let byte = ((1u16 << depth) as u8).wrapping_sub(1) << (8 - depth);
        let v = build(&ihdr(1, 1, depth, 0, 0), &[], &[0, byte]);
        assert_eq!(
            decode(&v).unwrap().get(0, 0),
            Some(0xFFFF_FFFF),
            "depth {depth} max {max}"
        );
        let v = build(&ihdr(1, 1, depth, 0, 0), &[], &[0, 0]);
        assert_eq!(decode(&v).unwrap().get(0, 0), Some(0xFF00_0000));
    }
}
