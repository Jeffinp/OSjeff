use super::*;

fn opaque(s: &Surface) -> usize {
    s.px.iter().filter(|p| (**p >> 24) > 200).count()
}

#[test]
fn sprites_have_content_inside_the_box() {
    for shape in [Shape::Arrow, Shape::Hand, Shape::IBeam] {
        let s = render(shape);
        assert_eq!((s.w, s.h), (W, H));
        assert!(opaque(&s) > 30, "{shape:?} is blank");
        let (hx, hy) = hotspot(shape);
        assert!((0..W as i32).contains(&hx) && (0..H as i32).contains(&hy));
    }
}

#[test]
fn hotspot_touches_the_shape() {
    // Within two pixels of the hotspot there is visible ink.
    for shape in [Shape::Arrow, Shape::Hand, Shape::IBeam] {
        let s = render(shape);
        let (hx, hy) = hotspot(shape);
        let mut best = 0;
        for dy in -2..=2i32 {
            for dx in -2..=2i32 {
                let (x, y) = ((hx + dx) as usize, (hy + dy) as usize);
                best = best.max(s.get(x, y) >> 24);
            }
        }
        assert!(best > 100, "{shape:?}: nothing at the hotspot");
    }
}

#[test]
fn the_box_edge_is_clear_so_nothing_is_clipped() {
    for shape in [Shape::Arrow, Shape::Hand, Shape::IBeam] {
        let s = render(shape);
        for x in 0..W {
            assert!((s.get(x, 0) >> 24) < 12 && (s.get(x, H - 1) >> 24) < 12);
        }
        for y in 0..H {
            assert!((s.get(0, y) >> 24) < 12 && (s.get(W - 1, y) >> 24) < 12);
        }
    }
}

#[test]
#[ignore]
fn dump_pointers() {
    // POINTER_SHEET=/tmp/p.ppm: the three sprites at 8x on light and dark.
    let Ok(path) = std::env::var("POINTER_SHEET") else {
        return;
    };
    let z = 8usize;
    let (w, h) = (3 * (W * z + 8), 2 * (H * z + 8));
    let mut img = std::vec![0u8; w * h * 3];
    for (row, bg) in [0xE8u32, 0x22].into_iter().enumerate() {
        for (i, shape) in [Shape::Arrow, Shape::Hand, Shape::IBeam]
            .into_iter()
            .enumerate()
        {
            let s = render(shape);
            for y in 0..H * z {
                for x in 0..W * z {
                    let p = s.px[(y / z) * W + x / z];
                    let a = p >> 24;
                    let o = ((row * (H * z + 8) + y) * w + i * (W * z + 8) + x) * 3;
                    for (k, sh) in [16, 8, 0].iter().enumerate() {
                        let c = (p >> sh) & 0xFF;
                        img[o + k] = (c + bg * (255 - a) / 255).min(255) as u8;
                    }
                }
            }
        }
    }
    let mut out = std::format!("P6\n{w} {h}\n255\n").into_bytes();
    out.extend_from_slice(&img);
    std::fs::write(path, out).unwrap();
}

#[test]
fn rendering_is_deterministic() {
    assert_eq!(render(Shape::Arrow).px, render(Shape::Arrow).px);
}
