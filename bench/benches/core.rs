//! Hot pure-logic functions of `kitsune_core`, on the host.
use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use kitsune_core::editor2::Editor;
use kitsune_core::heap::{adjust_request, fit_region};
use kitsune_core::shell::{Host, MemFs, MockSys, Shell};
use kitsune_core::system::input::{KeyCode, KeyEvent, Mods};
use kitsune_core::{Calc, Clipboard, Key, Keymap, browser, fs, net, web};

/// First-fit scan over a sorted free list of `n` small holes with the big
/// region last — the worst case of the kernel allocator's `find_region`.
fn bench_heap(c: &mut Criterion) {
    let mut g = c.benchmark_group("heap_find_region");
    for n in [8usize, 64, 512, 4096] {
        // (start, size): n holes of 64 bytes, 64 bytes apart, then one big region.
        let mut holes: Vec<(usize, usize)> = (0..n).map(|i| (0x1000 + i * 128, 64)).collect();
        holes.push((0x1000 + n * 128, 1 << 20));
        g.bench_with_input(BenchmarkId::new("scan_for_4KiB", n), &holes, |b, holes| {
            b.iter(|| {
                let (size, align) = adjust_request(4096, 8, 16, 8);
                holes
                    .iter()
                    .find_map(|&(s, l)| fit_region(s, l, black_box(size), align, 16))
            })
        });
    }
    g.bench_function("fit_region_single", |b| {
        b.iter(|| fit_region(black_box(0x1003), 4096, black_box(100), 16, 16))
    });
    g.finish();
}

fn bench_terminal(c: &mut Criterion) {
    let mut g = c.benchmark_group("terminal");
    g.bench_function("run_help", |b| {
        let (mut fs, mut sys) = (MemFs::new(), MockSys::default());
        let mut sh = Shell::new();
        b.iter(|| {
            let mut h = Host {
                fs: &mut fs,
                sys: &mut sys,
            };
            black_box(sh.run_line("help", &mut h))
        })
    });
    g.bench_function("echo_flood_100_lines", |b| {
        let (mut fs, mut sys) = (MemFs::new(), MockSys::default());
        let mut sh = Shell::new();
        b.iter(|| {
            for _ in 0..100 {
                let mut h = Host {
                    fs: &mut fs,
                    sys: &mut sys,
                };
                black_box(sh.run_line("echo a fairly long line of terminal output text!!", &mut h));
            }
        })
    });
    g.finish();
}

fn bench_editor_calc_keymap(c: &mut Criterion) {
    let mut g = c.benchmark_group("input_models");
    g.bench_function("editor_type_600_chars", |b| {
        b.iter(|| {
            let mut e = Editor::new();
            let mut clip = Clipboard::new();
            for i in 0..600u32 {
                let code = if i % 40 == 39 {
                    KeyCode::Enter
                } else {
                    KeyCode::Char((b'a' + (i % 26) as u8) as char)
                };
                e.handle_key(KeyEvent::new(code, Mods::NONE), &mut clip);
            }
            black_box(e.line_count())
        })
    });
    g.bench_function("calc_expression", |b| {
        b.iter(|| {
            let mut c = Calc::new();
            for &k in b"123+456*7=" {
                c.input(black_box(k));
            }
            black_box(c.display().len())
        })
    });
    g.bench_function("keymap_1000_scancodes", |b| {
        let mut km = Keymap::new();
        b.iter(|| {
            let mut n = 0usize;
            for i in 0..1000u32 {
                n += km.process((i % 58) as u8, false, i % 2 == 0).is_some() as usize;
            }
            black_box(n)
        })
    });
    g.finish();
}

fn page(paragraphs: usize) -> Vec<u8> {
    let mut s = String::from(
        "<html><head><style>body{background:#fff;color:#222} h1{color:#0d47a1} \
         .card{background:#eef;padding:8px} a{color:#15c}</style></head><body><h1>Title</h1>",
    );
    for i in 0..paragraphs {
        s.push_str(&format!(
            "<div class=\"card\"><h2>Section {i}</h2><p>Lorem ipsum dolor sit amet, \
             <b>consectetur</b> adipiscing elit &amp; sed do <a href=\"/x{i}\">eiusmod</a> \
             tempor incididunt ut labore et dolore magna aliqua.</p><ul><li>one</li>\
             <li>two</li><li>three</li></ul></div>"
        ));
    }
    s.push_str("</body></html>");
    s.into_bytes()
}

fn bench_web(c: &mut Criterion) {
    let mut g = c.benchmark_group("web_render");
    for n in [4usize, 40, 200] {
        let html = page(n);
        g.throughput(Throughput::Bytes(html.len() as u64));
        g.bench_with_input(
            BenchmarkId::new("html_to_displaylist", html.len()),
            &html,
            |b, h| b.iter(|| black_box(web::render(black_box(h), 880))),
        );
    }
    let html = page(40);
    g.bench_function("parse_html_only/40", |b| {
        b.iter(|| black_box(web::parse_html(black_box(&html))))
    });
    g.finish();

    let mut g = c.benchmark_group("browser");
    let mut resp = b"HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\r\n".to_vec();
    resp.extend_from_slice(&page(40));
    g.bench_function("page_body_40", |b| {
        b.iter(|| black_box(browser::page_body(black_box(&resp))))
    });
    g.bench_function("parse_url", |b| {
        b.iter(|| {
            black_box(browser::parse_url(black_box(
                b"https://example.com:8443/a/b?q=1",
            )))
        })
    });
    g.finish();
}

fn bench_net(c: &mut Criterion) {
    let mut g = c.benchmark_group("net_checksum");
    for len in [64usize, 1500, 16384] {
        let data: Vec<u8> = (0..len).map(|i| (i * 31) as u8).collect();
        g.throughput(Throughput::Bytes(len as u64));
        g.bench_with_input(BenchmarkId::from_parameter(len), &data, |b, d| {
            b.iter(|| black_box(net::checksum(black_box(d))))
        });
    }
    g.finish();
}

fn bench_fs(c: &mut Criterion) {
    let mut g = c.benchmark_group("fs");
    let mut img = vec![0u8; fs::IMAGE_SIZE];
    fs::format(&mut img);
    let data = vec![b'x'; fs::MAX_FILE_SIZE];
    for i in 0..30 {
        let name = format!("file{i}.txt");
        fs::write(&mut img, name.as_bytes(), &data).unwrap();
    }
    g.bench_function("find_file_29", |b| {
        b.iter(|| black_box(fs::find(&img, black_box(b"file29.txt"))))
    });
    g.bench_function("read_1KiB", |b| {
        b.iter(|| black_box(fs::read(&img, black_box(b"file15.txt"))))
    });
    g.bench_function("write_1KiB_overwrite", |b| {
        b.iter(|| black_box(fs::write(&mut img, b"file15.txt", black_box(&data))))
    });
    g.bench_function("count_active", |b| {
        b.iter(|| black_box(fs::count_active(&img)))
    });
    g.finish();
}

criterion_group!(
    benches,
    bench_heap,
    bench_terminal,
    bench_editor_calc_keymap,
    bench_web,
    bench_net,
    bench_fs
);
criterion_main!(benches);
