//! Framebuffer primitives, baseline (`fb_old`) vs current (`fb`), on the host.
//! Same sizes as the in-kernel `trace::bench_prims` (a 512x320 window, its
//! 520x324 shadow layer, a 48-char string at scale 2).
use bootloader_api::info::{FrameBufferInfo, PixelFormat};
use criterion::{Criterion, black_box, criterion_group, criterion_main};
use kitsune_bench::{fb, fb_old, font, font_old};

fn info(bpp: usize, h: usize) -> FrameBufferInfo {
    FrameBufferInfo {
        byte_len: 1280 * h * bpp,
        width: 1280,
        height: h,
        pixel_format: PixelFormat::Bgr,
        bytes_per_pixel: bpp,
        stride: 1280,
    }
}

fn buf(n: usize) -> Vec<u8> {
    vec![0x40; n + 64]
}

const TEXT: &str = "The quick brown fox jumps over the lazy dog 0123";

fn bench_fb(c: &mut Criterion) {
    for (bpp, h) in [(3usize, 720usize), (4, 800)] {
        let inf = info(bpp, h);
        let mut g = c.benchmark_group(format!("fb_bpp{bpp}"));
        let mut b_old = buf(inf.byte_len);
        let mut b_new = buf(inf.byte_len);
        let col_old = fb_old::Color::rgb(0x20, 0x40, 0x80);
        let col_new = fb::Color::rgb(0x20, 0x40, 0x80);

        g.bench_function("fill_rect_512x320/old", |b| {
            let mut cv = fb_old::Canvas::new(&mut b_old[..inf.byte_len], inf);
            b.iter(|| cv.fill_rect(100, 100, 512, 320, black_box(col_old)));
        });
        g.bench_function("fill_rect_512x320/new", |b| {
            let mut cv = fb::Canvas::new(&mut b_new[..inf.byte_len], inf);
            b.iter(|| cv.fill_rect(100, 100, 512, 320, black_box(col_new)));
        });
        g.bench_function("alpha_520x324/old", |b| {
            let mut cv = fb_old::Canvas::new(&mut b_old[..inf.byte_len], inf);
            b.iter(|| cv.fill_round_rect_alpha(100, 100, 520, 324, 18, black_box(col_old), 28));
        });
        g.bench_function("alpha_520x324/new", |b| {
            let mut cv = fb::Canvas::new(&mut b_new[..inf.byte_len], inf);
            b.iter(|| cv.fill_round_rect_alpha(100, 100, 520, 324, 18, black_box(col_new), 28));
        });
        g.bench_function("text_48ch_x2/old", |b| {
            let mut cv = fb_old::Canvas::new(&mut b_old[..inf.byte_len], inf);
            b.iter(|| font_old::draw_text(&mut cv, 100, 100, black_box(TEXT), col_old, 2));
        });
        g.bench_function("text_48ch_x2/new", |b| {
            let mut cv = fb::Canvas::new(&mut b_new[..inf.byte_len], inf);
            b.iter(|| font::draw_text(&mut cv, 100, 100, black_box(TEXT), col_new, 2));
        });
        g.finish();
    }
}

criterion_group!(benches, bench_fb);
criterion_main!(benches);
