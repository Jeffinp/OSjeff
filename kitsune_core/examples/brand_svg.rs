//! Writes the brand assets from the polygon data of `kitsune_core::brand`:
//!
//! ```text
//! cargo run -p kitsune_core --example brand_svg                # SVGs into docs/brand/
//! cargo run -p kitsune_core --example brand_svg -- OUT_DIR     # ... into OUT_DIR
//! cargo run -p kitsune_core --example brand_svg -- OUT_DIR --ppm
//!     # also writes sizes.ppm and social-preview.ppm (convert them to PNG with ImageMagick:
//!     #   convert sizes.ppm sizes.png; convert social-preview.ppm social-preview.png)
//! ```
//!
//! The SVGs are the same polygons the kernel rasterises, so the files in `docs/brand/` can
//! never drift from the UI: regenerate them whenever `brand.rs` changes (see the golden hash
//! test in `src/brand/tests.rs`).

use kitsune_core::brand::{self, Scheme};
use kitsune_core::fontcache::{TextEngine, Weight};
use kitsune_core::raster::{Surface, over, unpremul};
use std::path::Path;

const S: Scheme = Scheme::KITSUNE;

const REGULAR: &[u8] = include_bytes!("../../assets/fonts/Inter-Regular.subset.ttf");
const MEDIUM: &[u8] = include_bytes!("../../assets/fonts/Inter-Medium.subset.ttf");
const SEMIBOLD: &[u8] = include_bytes!("../../assets/fonts/Inter-SemiBold.subset.ttf");
const MONO: &[u8] = include_bytes!("../../assets/fonts/JetBrainsMono-Regular.subset.ttf");

fn write_ppm(path: &Path, s: &Surface) {
    let mut out = format!("P6\n{} {}\n255\n", s.w, s.h).into_bytes();
    for &p in &s.px {
        let c = unpremul(p);
        out.extend_from_slice(&[(c >> 16) as u8, (c >> 8) as u8, c as u8]);
    }
    std::fs::write(path, out).expect("write ppm");
}

#[allow(clippy::too_many_arguments)]
fn text(
    s: &mut Surface,
    eng: &mut TextEngine,
    x: i32,
    baseline: i32,
    t: &str,
    px: u16,
    w: Weight,
    rgb: u32,
) {
    let mut pen = x * 256;
    for c in t.chars() {
        let g = eng.glyph(w, px, c);
        let cov = eng.coverage(&g).to_vec();
        for gy in 0..g.h as i32 {
            for gx in 0..g.w as i32 {
                let a = cov[(gy * g.w as i32 + gx) as usize] as u32;
                if a == 0 {
                    continue;
                }
                let (dx, dy) = (pen / 256 + g.left as i32 + gx, baseline - g.top as i32 + gy);
                if dx < 0 || dy < 0 || dx as usize >= s.w || dy as usize >= s.h {
                    continue;
                }
                let i = dy as usize * s.w + dx as usize;
                s.px[i] = over(s.px[i], kitsune_core::raster::premul((a << 24) | rgb));
            }
        }
        pen += g.adv_q8;
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = args.next().unwrap_or_else(|| "docs/brand".into());
    let ppm = args.any(|a| a == "--ppm");
    let dir = Path::new(&dir);
    std::fs::create_dir_all(dir).expect("create the output directory");

    let files = [
        (
            "kitsune-tile.svg",
            brand::to_svg(&brand::tile(&S), "Kitsune"),
        ),
        (
            "kitsune-mono.svg",
            brand::to_svg(&brand::mono(0x2A2A5C), "Kitsune (monochrome)"),
        ),
        (
            "kitsune-halo.svg",
            brand::to_svg(&brand::halo(&S), "Kitsune (with tails)"),
        ),
        (
            "kitsune-halo-9.svg",
            brand::to_svg(&brand::halo_n(&S, 9), "Kitsune (nine tails)"),
        ),
    ];
    for (name, svg) in &files {
        std::fs::write(dir.join(name), svg).expect("write svg");
        println!("wrote {}", dir.join(name).display());
    }

    if ppm {
        // The tuned sizes side by side on a dark and a light background (docs/brand/sizes.png).
        let sizes = [16usize, 24, 32, 48, 64, 128];
        let w: usize = sizes.iter().map(|p| p + 24).sum::<usize>() + 24;
        let mut sheet = Surface::new(w, 2 * 128 + 3 * 24);
        for (row, bg) in [(0usize, 0xFF14141Fu32), (1, 0xFFF3F3F7)] {
            let mut band = Surface::new(w, 128 + 24);
            band.fill(bg);
            sheet.blit(&band, 0, (row * (128 + 24 + 12)) as i32, 256);
        }
        for row in 0..2usize {
            let mut x = 24;
            for px in sizes {
                let y = (row * (128 + 24 + 12) + 12 + (128 - px) / 2) as i32;
                sheet.blit(&brand::render(&brand::tile(&S), px), x, y, 256);
                x += px as i32 + 24;
            }
        }
        write_ppm(&dir.join("sizes.ppm"), &sheet);

        // GitHub social preview: 1280 x 640.
        let mut eng = TextEngine::new([REGULAR, MEDIUM, SEMIBOLD, MONO]).expect("fonts");
        let mut card = Surface::new(1280, 640);
        card.fill(0xFF1D1D45);
        let mark = brand::render(&brand::halo(&S), 420);
        card.blit(&mark, 90, 110, 256);
        text(
            &mut card,
            &mut eng,
            560,
            330,
            "Kitsune",
            120,
            Weight::Semibold,
            0xFFF3E6,
        );
        text(
            &mut card,
            &mut eng,
            566,
            400,
            "An operating system written from scratch.",
            34,
            Weight::Regular,
            0xC4C6E8,
        );
        write_ppm(&dir.join("social-preview.ppm"), &card);
        println!("wrote sizes.ppm and social-preview.ppm");
    }
}
