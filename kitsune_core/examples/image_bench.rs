//! Host benchmark for the image stack (not part of the kernel build).
//!
//! ```text
//! cargo run --release -p kitsune_core --example image_bench [-- [--dump DIR] [FILE.png]]
//! ```
//!
//! Without a file it builds a deterministic 1024x768 RGBA test picture
//! (gradient + circles + noise) and benchmarks everything on it. With a PNG
//! path it also benchmarks decoding that file (e.g. one written by libpng, to
//! measure real dynamic-Huffman streams). `--dump DIR` writes our encoders'
//! output (PNG, BMP24, BMP32) so other tools can cross-check it.

use kitsune_core::bmp;
use kitsune_core::image::{Filter, Image, rgba};
use kitsune_core::inflate::{adler32, crc32, zlib_decompress};
use kitsune_core::png;
use std::time::{Duration, Instant};

const W: usize = 1024;
const H: usize = 768;

fn picture() -> Image {
    let mut x = 0x1234_5678u32;
    let mut px = Vec::with_capacity(W * H);
    for y in 0..H {
        for xx in 0..W {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            let noise = (x >> 28) as i32; // 0..15
            let mut r = (xx * 255 / W) as i32;
            let mut g = (y * 255 / H) as i32;
            let mut b = ((xx + y) * 255 / (W + H)) as i32;
            // A few filled circles with hard edges.
            for (cx, cy, rad, col) in [
                (300i32, 250i32, 120i32, 200i32),
                (700, 500, 200, 60),
                (900, 150, 60, 255),
            ] {
                let (dx, dy) = (xx as i32 - cx, y as i32 - cy);
                if dx * dx + dy * dy < rad * rad {
                    r = col;
                    g = 255 - col;
                    b = col / 2;
                }
            }
            let a = if (xx / 64 + y / 64) % 5 == 0 {
                128
            } else {
                255
            };
            px.push(rgba(
                (r + noise).clamp(0, 255) as u8,
                (g + noise).clamp(0, 255) as u8,
                (b + noise / 2).clamp(0, 255) as u8,
                a,
            ));
        }
    }
    Image::from_pixels(W, H, px).unwrap()
}

/// Runs `f` several times and reports the best and median time.
fn bench<T>(name: &str, bytes: usize, reps: usize, mut f: impl FnMut() -> T) -> T {
    let mut times: Vec<Duration> = Vec::new();
    let mut out = f(); // warm-up
    for _ in 0..reps {
        let t = Instant::now();
        out = std::hint::black_box(f());
        times.push(t.elapsed());
    }
    times.sort();
    let best = times[0];
    let med = times[times.len() / 2];
    let mbs = |d: Duration| bytes as f64 / d.as_secs_f64() / 1e6;
    println!(
        "{name:<34} best {:>8.2} ms  median {:>8.2} ms  {:>8.1} MB/s (of {} KiB)",
        best.as_secs_f64() * 1e3,
        med.as_secs_f64() * 1e3,
        mbs(best),
        bytes / 1024
    );
    out
}

fn main() {
    let mut dump: Option<String> = None;
    let mut file: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--dump" {
            dump = args.next();
        } else {
            file = Some(a);
        }
    }
    let img = picture();
    let raw_bytes = W * H * 4;
    println!("image {W}x{H} RGBA ({} KiB raw)", raw_bytes / 1024);

    println!("-- checksums");
    let rgba_bytes = img.to_rgba().unwrap();
    bench("crc32", raw_bytes, 20, || crc32(&rgba_bytes));
    bench("adler32", raw_bytes, 20, || adler32(&rgba_bytes));

    println!("-- png (ours: LZ77 + fixed Huffman)");
    let ours = bench("png encode_rgba", raw_bytes, 3, || {
        png::encode_rgba(&img).unwrap()
    });
    println!(
        "   encoded size {} KiB ({:.1}% of raw)",
        ours.len() / 1024,
        100.0 * ours.len() as f64 / raw_bytes as f64
    );
    let dec = bench("png decode (our stream)", raw_bytes, 15, || {
        png::decode(&ours).unwrap()
    });
    assert_eq!(dec, img);
    let opaque = {
        let mut o = img.clone();
        o.flatten(0xFF00_0000);
        o
    };
    let ours_rgb = bench("png encode (opaque -> RGB8)", raw_bytes, 3, || {
        png::encode(&opaque).unwrap()
    });
    println!("   encoded size {} KiB", ours_rgb.len() / 1024);
    bench("png decode (RGB8 stream)", raw_bytes, 15, || {
        png::decode(&ours_rgb).unwrap()
    });

    if let Some(path) = &file {
        let data = std::fs::read(path).expect("read png");
        let hdr = png::read_header(&data).unwrap();
        println!(
            "-- png file {path}: {}x{} depth {} {:?} interlaced {}, {} KiB",
            hdr.width,
            hdr.height,
            hdr.bit_depth,
            hdr.color_type,
            hdr.interlaced,
            data.len() / 1024
        );
        let px = hdr.width as usize * hdr.height as usize * 4;
        let decoded = bench("png decode (file)", px, 15, || png::decode(&data).unwrap());
        if let Some(dir) = &dump {
            std::fs::create_dir_all(dir).unwrap();
            std::fs::write(
                format!("{dir}/file_decoded.raw"),
                decoded.to_rgba().unwrap(),
            )
            .unwrap();
        }
    }

    println!("-- inflate (raw zlib payload of our stream)");
    let mut idat = Vec::new();
    {
        // Concatenate IDAT payloads by walking the chunks.
        let mut pos = 8;
        while pos + 12 <= ours.len() {
            let len = u32::from_be_bytes(ours[pos..pos + 4].try_into().unwrap()) as usize;
            if &ours[pos + 4..pos + 8] == b"IDAT" {
                idat.extend_from_slice(&ours[pos + 8..pos + 8 + len]);
            }
            pos += 12 + len;
        }
    }
    let inflated_len = zlib_decompress(&idat, 1 << 28).unwrap().len();
    bench("zlib_decompress", inflated_len, 15, || {
        zlib_decompress(&idat, 1 << 28).unwrap()
    });

    println!("-- resampling");
    bench("resize_bilinear 1024x768 -> 512x384", raw_bytes, 15, || {
        img.resize_bilinear(512, 384).unwrap()
    });
    bench(
        "resize_bilinear 1024x768 -> 1280x960",
        raw_bytes,
        15,
        || img.resize_bilinear(1280, 960).unwrap(),
    );
    bench("resize_bilinear (opaque) -> 512x384", raw_bytes, 15, || {
        opaque.resize_bilinear(512, 384).unwrap()
    });
    bench(
        "resize_bilinear (opaque) -> 1280x960",
        raw_bytes,
        15,
        || opaque.resize_bilinear(1280, 960).unwrap(),
    );
    bench("resize_box 1024x768 -> 256x192", raw_bytes, 15, || {
        img.resize_box(256, 192).unwrap()
    });
    bench(
        "resize_box 1024x768 -> 320x240 (thumb)",
        raw_bytes,
        15,
        || img.fit(320, 240, false, Filter::Auto).unwrap(),
    );
    bench("resize_nearest 1024x768 -> 512x384", raw_bytes, 15, || {
        img.resize_nearest(512, 384).unwrap()
    });
    bench("rotate90", raw_bytes, 15, || img.rotate90().unwrap());
    bench("flatten over background", raw_bytes, 15, || {
        let mut c = img.clone();
        c.flatten(0xFF10_2030);
        c
    });

    println!("-- bmp");
    let b24 = bench("bmp encode_24", raw_bytes, 10, || {
        bmp::encode_24(&img, 0).unwrap()
    });
    let b32 = bench("bmp encode_32", raw_bytes, 10, || {
        bmp::encode_32(&img).unwrap()
    });
    bench("bmp decode 24", raw_bytes, 15, || {
        bmp::decode(&b24).unwrap()
    });
    bench("bmp decode 32", raw_bytes, 15, || {
        bmp::decode(&b32).unwrap()
    });

    if let Some(dir) = dump {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(format!("{dir}/ours_rgba.png"), &ours).unwrap();
        std::fs::write(format!("{dir}/ours_rgb.png"), &ours_rgb).unwrap();
        std::fs::write(
            format!("{dir}/ours24.bmp"),
            bmp::encode_24(&opaque, 0).unwrap(),
        )
        .unwrap();
        std::fs::write(format!("{dir}/ours32.bmp"), &b32).unwrap();
        std::fs::write(format!("{dir}/expected_rgba.raw"), &rgba_bytes).unwrap();
        std::fs::write(
            format!("{dir}/expected_opaque_rgba.raw"),
            opaque.to_rgba().unwrap(),
        )
        .unwrap();
        println!("wrote encoder output to {dir}");
    }
}
