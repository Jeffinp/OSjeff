use super::*;

#[test]
fn over_fast_paths() {
    assert_eq!(over(RED, BLUE), RED); // opaque source wins
    assert_eq!(over(0x00FF_FFFF, BLUE), BLUE); // transparent source
    assert_eq!(over(0x0000_0000, 0), 0);
}

#[test]
fn over_half_alpha_on_opaque() {
    let o = over(0x80FF_0000, BLACK);
    assert_eq!(alpha(o), 255);
    assert_eq!(channels(o)[0], 128);
    let o = over(0x80FF_FFFF, BLACK);
    assert_eq!(channels(o)[..3], [128, 128, 128]);
    // 255 - a of the destination plus a of the source, rounded.
    let o = over(0x4000_00FF, WHITE);
    assert_eq!(channels(o), [191, 191, 255, 255]);
}

#[test]
fn over_onto_transparent_destination_keeps_source_colour() {
    let o = over(0x80C8_6414, 0x0000_0000);
    assert_eq!(channels(o), [200, 100, 20, 128]);
    // Two half-transparent layers: alpha 0.5 + 0.5*0.5 = 0.75.
    let o = over(0x80FF_0000, 0x800000FF);
    assert!((190..=192).contains(&alpha(o)), "alpha {}", alpha(o));
    let [r, _, b, _] = channels(o);
    assert!(r > b); // the top layer dominates
}

#[test]
fn over_never_overflows_a_channel() {
    for sa in (0..=255u32).step_by(15) {
        for da in (0..=255u32).step_by(15) {
            for c in [0u8, 1, 127, 128, 254, 255] {
                let s = rgba(c, c, c, sa as u8);
                let d = rgba(255 - c, c, 255, da as u8);
                let o = over(s, d);
                let [r, g, b, a] = channels(o);
                // Result colour lies between the two inputs per channel.
                let lo = |x: u8, y: u8| x.min(y);
                let hi = |x: u8, y: u8| x.max(y);
                if a > 0 && sa > 0 && da > 0 {
                    assert!(
                        r >= lo(c, 255 - c).saturating_sub(1)
                            && r <= hi(c, 255 - c).saturating_add(1)
                    );
                    assert!(g >= c.saturating_sub(1) && g <= c.saturating_add(1));
                    assert!(b >= lo(c, 255).saturating_sub(1));
                }
            }
        }
    }
}

#[test]
fn flatten_composites_over_a_background() {
    let mut i = img(3, 1, &[RED, 0x0000_FF00, 0x80FF_FFFF]);
    i.flatten(0x0000_00FF); // background alpha is ignored: treated as opaque
    assert_eq!(i.pixels()[0], RED);
    assert_eq!(i.pixels()[1], BLUE);
    let [r, g, b, a] = channels(i.pixels()[2]);
    assert_eq!(a, 255);
    assert_eq!((r, g), (128, 128));
    assert_eq!(b, 255);
    assert!(i.is_opaque());
}

#[test]
fn blit_over_clips_on_every_side() {
    let mut dst = Image::new(4, 4, BLACK).unwrap();
    let src = Image::new(2, 2, WHITE).unwrap();
    dst.blit_over(&src, 1, 1);
    assert_eq!(dst.get(1, 1), Some(WHITE));
    assert_eq!(dst.get(2, 2), Some(WHITE));
    assert_eq!(dst.get(0, 0), Some(BLACK));
    assert_eq!(dst.get(3, 3), Some(BLACK));
    // Hanging off the top-left corner: only the bottom-right source pixel lands.
    let mut d = Image::new(4, 4, BLACK).unwrap();
    d.blit_over(&src, -1, -1);
    assert_eq!(d.get(0, 0), Some(WHITE));
    assert_eq!(d.get(1, 0), Some(BLACK));
    // Off the bottom-right.
    let mut d = Image::new(4, 4, BLACK).unwrap();
    d.blit_over(&src, 3, 3);
    assert_eq!(d.get(3, 3), Some(WHITE));
    assert_eq!(d.get(2, 3), Some(BLACK));
    // Completely outside, including extreme offsets.
    let before = d.clone();
    d.blit_over(&src, 4, 0);
    d.blit_over(&src, 0, 4);
    d.blit_over(&src, -2, 0);
    d.blit_over(&src, i32::MIN, i32::MIN);
    d.blit_over(&src, i32::MAX, i32::MAX);
    assert_eq!(d, before);
}

#[test]
fn blit_over_blends_alpha() {
    let mut dst = Image::new(2, 1, BLACK).unwrap();
    let src = img(2, 1, &[0x80FF_FFFF, 0x0000_0000]);
    dst.blit_over(&src, 0, 0);
    assert_eq!(channels(dst.get(0, 0).unwrap())[0], 128);
    assert_eq!(dst.get(1, 0), Some(BLACK));
}

#[test]
fn blit_larger_source_into_smaller_destination() {
    let mut dst = Image::new(2, 2, BLACK).unwrap();
    let src = Image::new(10, 10, RED).unwrap();
    dst.blit_over(&src, -3, -3);
    assert!(dst.pixels().iter().all(|&p| p == RED));
}

#[test]
fn error_messages_are_nonempty() {
    use alloc::string::ToString;
    for e in [
        ImageError::ZeroSize,
        ImageError::TooLarge,
        ImageError::BadBuffer,
        ImageError::OutOfBounds,
        ImageError::OutOfMemory,
    ] {
        assert!(!e.to_string().is_empty());
    }
}
