use super::*;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec;
use core::fmt::Write;

type Ring = LogRing<512>;

fn snap<const N: usize>(r: &LogRing<N>) -> Vec<u8> {
    let mut v = vec![0u8; r.used()];
    assert_eq!(r.copy_out(&mut v), r.used());
    v
}

#[test]
fn push_and_read_back() {
    let mut r = Ring::new();
    assert_eq!(r.push(10, Level::Info, 0, b"hello"), 0);
    assert_eq!(r.push(20, Level::Error, 2, b"world!"), 1);
    let s = snap(&r);
    let v: Vec<_> = records(&s).collect();
    assert_eq!(v.len(), 2);
    assert_eq!(v[0].text, b"hello");
    assert_eq!((v[0].ts_ms, v[0].level, v[0].origin), (10, Level::Info, 0));
    assert_eq!(v[1].text, b"world!");
    assert_eq!(v[1].level, Level::Error);
    assert_eq!(r.next_seq(), 2);
}

#[test]
fn long_text_is_cut() {
    let mut r = Ring::new();
    r.push(0, Level::Info, 0, &[b'x'; 500]);
    let s = snap(&r);
    assert_eq!(records(&s).next().unwrap().text.len(), MAX_MSG);
}

#[test]
fn oldest_records_are_dropped_and_order_is_kept() {
    let mut r = Ring::new();
    for i in 0..100u32 {
        let mut m = String::new();
        let _ = write!(m, "message number {i}");
        r.push(i, Level::Info, 0, m.as_bytes());
    }
    assert!(r.dropped() > 0);
    let s = snap(&r);
    let v: Vec<_> = records(&s).collect();
    // Strictly consecutive sequence numbers ending at the newest.
    for w in v.windows(2) {
        assert_eq!(w[1].seq, w[0].seq + 1);
    }
    assert_eq!(v.last().unwrap().seq, 99);
    assert_eq!(r.dropped() as usize + v.len(), 100);
    for e in &v {
        let mut m = String::new();
        let _ = write!(m, "message number {}", e.seq);
        assert_eq!(e.text, m.as_bytes());
    }
}

#[test]
fn hundred_thousand_messages_never_corrupt_the_ring() {
    // A 64 KiB ring like the kernel's, hammered with variable-size texts
    // that straddle the wrap point many times over.
    let mut r: Box<LogRing<RING_BYTES>> = Box::default();
    let mut seed = 0x1234_5678u32;
    for i in 0..100_000u32 {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let len = (seed >> 24) as usize % 120;
        let mut text = vec![b'a' + (i % 26) as u8; len];
        if len >= 4 {
            text[..4].copy_from_slice(&i.to_le_bytes());
        }
        r.push(i, Level::from_u8((i % 6) as u8), (i % 8) as u8, &text);
        // Spot-check the invariants cheaply, and fully now and then.
        if i % 9973 == 0 {
            let s = snap(&*r);
            let mut prev: Option<u32> = None;
            let mut bytes = 0;
            for e in records(&s) {
                if let Some(p) = prev {
                    assert_eq!(e.seq, p + 1);
                }
                prev = Some(e.seq);
                assert_eq!(e.ts_ms, e.seq);
                if e.text.len() >= 4 {
                    assert_eq!(&e.text[..4], &e.seq.to_le_bytes());
                }
                bytes += HDR + e.text.len();
            }
            assert_eq!(bytes, s.len());
            assert_eq!(prev, Some(i));
        }
    }
    assert_eq!(r.next_seq(), 100_000);
    let s = snap(&*r);
    assert_eq!(records(&s).last().unwrap().seq, 99_999);
    assert_eq!(r.dropped() as usize + records(&s).count(), 100_000);
}

#[test]
fn copy_out_needs_room() {
    let mut r = Ring::new();
    r.push(0, Level::Info, 0, b"abc");
    let mut small = [0u8; 4];
    assert_eq!(r.copy_out(&mut small), 0);
}

#[test]
fn clear_empties_but_keeps_counting() {
    let mut r = Ring::new();
    r.push(0, Level::Info, 0, b"a");
    r.clear();
    assert_eq!(r.used(), 0);
    assert_eq!(r.push(0, Level::Info, 0, b"b"), 1);
    let s = snap(&r);
    assert_eq!(records(&s).count(), 1);
}

#[test]
fn for_each_since_filters_by_seq_and_level() {
    let mut r = Ring::new();
    for i in 0..10u32 {
        r.push(i, Level::from_u8((i % 6) as u8), 0, b"m");
    }
    let mut got = Vec::new();
    r.for_each_since(5, Level::Warn, |e| got.push((e.seq, e.level)));
    assert_eq!(got, vec![(5, Level::Fatal), (9, Level::Warn)]);
}

#[test]
fn for_each_since_sees_wrapped_records() {
    let mut r: LogRing<256> = LogRing::new();
    for i in 0..40u32 {
        r.push(i, Level::Info, 0, b"wrapping text");
    }
    let mut n = 0;
    r.for_each_since(0, Level::Trace, |e| {
        assert_eq!(e.text, b"wrapping text");
        n += 1;
    });
    assert_eq!(n as u32 + r.dropped(), 40);
}

#[test]
fn level_helpers() {
    assert!(Level::Warn > Level::Info);
    assert_eq!(Level::from_u8(99), Level::Fatal);
    assert_eq!(Level::Fatal.next(), Level::Fatal);
    assert_eq!(Level::Info.tag(), "INFO ");
    for (i, l) in Level::ALL.iter().enumerate() {
        assert_eq!(*l as usize, i);
    }
}

#[test]
fn contains_ci_cases() {
    assert!(contains_ci(b"Hello World", b"o w"));
    assert!(contains_ci(b"Hello", b""));
    assert!(contains_ci(b"TSC calibrated", b"tsc"));
    assert!(!contains_ci(b"abc", b"abcd"));
    assert!(!contains_ci(b"abc", b"x"));
}

#[test]
fn filter_level_and_text() {
    let mut r = Ring::new();
    r.push(0, Level::Info, 0, b"net: lease");
    r.push(1, Level::Warn, 0, b"disk slow");
    r.push(2, Level::Error, 0, b"net: down");
    let s = snap(&r);
    let mut f = Filter::new();
    let mut v = LogView::new();
    v.rebuild(&s, &f, 10);
    assert_eq!(v.len(), 3);
    f.min = Level::Warn;
    v.rebuild(&s, &f, 10);
    assert_eq!(v.len(), 2);
    for c in b"NET" {
        assert!(f.push_char(*c));
    }
    v.rebuild(&s, &f, 10);
    assert_eq!(v.len(), 1);
    assert_eq!(v.visible(&s, 10).next().unwrap().text, b"net: down");
    assert!(f.backspace());
    assert_eq!(f.needle(), b"NE");
    f.clear_needle();
    assert!(!f.backspace());
    f.min = Level::Fatal;
    f.cycle_level();
    assert_eq!(f.min, Level::Trace);
}

#[test]
fn filter_rejects_control_and_overflow() {
    let mut f = Filter::new();
    assert!(!f.push_char(7));
    assert!(!f.push_char(0x80));
    for _ in 0..24 {
        assert!(f.push_char(b'a'));
    }
    assert!(!f.push_char(b'a'));
}

#[test]
fn view_follows_and_scrolls() {
    let mut r = Ring::new();
    for i in 0..20u32 {
        r.push(i, Level::Info, 0, b"line");
    }
    let s = snap(&r);
    let f = Filter::new();
    let mut v = LogView::new();
    v.rebuild(&s, &f, 5);
    assert_eq!(v.top(), 15); // following: bottom
    v.scroll(-3, 5);
    assert_eq!(v.top(), 12);
    assert!(!v.follow);
    // New data does not move a scrolled-up view.
    r.push(99, Level::Info, 0, b"new");
    let s = snap(&r);
    v.rebuild(&s, &f, 5);
    assert_eq!(v.top(), 12);
    v.scroll(100, 5);
    assert!(v.follow);
    assert_eq!(v.top(), v.len() - 5);
    v.home();
    assert_eq!(v.top(), 0);
    v.end(5);
    assert!(v.follow);
    let vis: Vec<_> = v.visible(&s, 5).collect();
    assert_eq!(vis.len(), 5);
    assert_eq!(vis.last().unwrap().text, b"new");
}

#[test]
fn view_with_fewer_lines_than_rows() {
    let mut r = Ring::new();
    r.push(0, Level::Info, 0, b"only");
    let s = snap(&r);
    let mut v = LogView::new();
    v.rebuild(&s, &Filter::new(), 8);
    assert_eq!(v.top(), 0);
    v.scroll(5, 8);
    assert_eq!(v.top(), 0);
    v.scroll(-5, 8);
    assert_eq!(v.top(), 0);
}

#[test]
fn prefix_format() {
    let e = Entry {
        seq: 0,
        ts_ms: 12_345,
        level: Level::Warn,
        origin: 0,
        text: b"",
    };
    let mut p = [0u8; PREFIX_LEN];
    let n = format_prefix(&e, &mut p);
    assert_eq!(&p[..n], b"   12.345 W ");
    let e = Entry { ts_ms: 7, ..e };
    format_prefix(&e, &mut p);
    assert_eq!(&p[..], b"    0.007 W ");
    let e = Entry {
        ts_ms: 99_999_999,
        ..e
    };
    format_prefix(&e, &mut p);
    assert_eq!(&p[..], b"99999.999 W ");
}

#[test]
fn render_text_dump() {
    let mut r = Ring::new();
    r.push(1500, Level::Info, 0, b"boot");
    r.push(2500, Level::Debug, 1, b"noise");
    let s = snap(&r);
    let mut f = Filter::new();
    f.min = Level::Info;
    let mut out = Vec::new();
    render_text(
        &s,
        &f,
        |o| if o == 0 { "kernel" } else { "other" },
        &mut out,
    );
    assert_eq!(out, b"    1.500 I kernel boot\n");
}

#[test]
fn dump_bounded_keeps_every_level_and_the_newest_whole_lines() {
    let mut r = Ring::new();
    r.push(10, Level::Trace, 0, b"first");
    r.push(20, Level::Debug, 0, b"second");
    r.push(30, Level::Error, 1, b"third");
    let s = snap(&r);
    let name = |o: u8| if o == 0 { "kernel" } else { "appd" };
    // Plenty of room: everything, in order, nothing dropped.
    let (all, cut) = dump_bounded(&s, name, 1 << 16);
    assert!(!cut);
    assert_eq!(
        all,
        b"    0.010 T kernel first\n    0.020 D kernel second\n    0.030 E appd third\n"
    );
    // Tight: only the newest whole lines, starting on a line boundary.
    let line = b"    0.030 E appd third\n".len();
    let (tail, cut) = dump_bounded(&s, name, line + 3);
    assert!(cut);
    assert_eq!(tail, b"    0.030 E appd third\n");
    // Never above the cap, whatever the cap.
    for cap in 0..all.len() + 4 {
        let (t, _) = dump_bounded(&s, name, cap);
        assert!(t.len() <= cap, "cap {cap}: {}", t.len());
        assert!(t.is_empty() || t.ends_with(b"\n"));
    }
    // An empty log is an empty file.
    let (none, cut) = dump_bounded(&[], name, 100);
    assert!(none.is_empty() && !cut);
}

#[test]
fn a_full_ring_dumps_within_the_boot_log_bound() {
    // The kernel's ring (64 KiB of records) rendered with prefixes and thread
    // names stays a bounded file: the flush never writes more than its cap.
    let mut r: Box<LogRing<RING_BYTES>> = Box::default();
    for i in 0..5000u32 {
        let mut m = String::new();
        let _ = write!(m, "storage: line {i} with some typical boot text in it");
        r.push(i, Level::Info, (i % 4) as u8, m.as_bytes());
    }
    let s = snap(&*r);
    let (t, cut) = dump_bounded(&s, |_| "kernel", 48 * 1024);
    assert!(
        cut && t.len() <= 48 * 1024 && t.len() > 40 * 1024,
        "{}",
        t.len()
    );
    assert!(t.ends_with(b"\n"));
    // The newest message is the last line.
    assert!(
        core::str::from_utf8(&t)
            .unwrap()
            .lines()
            .last()
            .unwrap()
            .contains("line 4999 ")
    );
}

#[test]
fn view_can_start_anywhere() {
    let mut ring = LogRing::<4096>::new();
    for i in 0..10u32 {
        let mut m = FixedBuf::<16>::new();
        let _ = core::fmt::Write::write_fmt(&mut m, format_args!("line {i}"));
        ring.push(i * 10, Level::Info, 0, m.as_bytes());
    }
    let mut snap = alloc::vec![0u8; ring.used()];
    assert_eq!(ring.copy_out(&mut snap), ring.used());
    let mut view = LogView::new();
    view.rebuild(&snap, &Filter::new(), 4);
    let texts = |first, n| {
        view.visible_from(&snap, first, n)
            .map(|e| e.text.to_vec())
            .collect::<Vec<_>>()
    };
    assert_eq!(texts(0, 2), [b"line 0".to_vec(), b"line 1".to_vec()]);
    assert_eq!(texts(8, 5), [b"line 8".to_vec(), b"line 9".to_vec()]);
    assert!(texts(10, 3).is_empty());
    assert_eq!(view.max_top_for(4), 6);
}

#[test]
fn fixed_buf_displays_utf8_and_falls_back_to_latin1() {
    let mut b = FixedBuf::<16>::new();
    let _ = core::fmt::Write::write_str(&mut b, "há 4 s");
    assert_eq!(alloc::format!("{b}"), "há 4 s");
    let mut raw = FixedBuf::<16>::new();
    raw.push_flat(&[b'a', 0xE9, b'b']);
    assert_eq!(alloc::format!("{raw}"), "a\u{e9}b");
    // Cut in the middle of a two-byte letter: no panic, each byte shown.
    let mut cut = FixedBuf::<2>::new();
    let _ = core::fmt::Write::write_str(&mut cut, "há");
    let _ = alloc::format!("{cut}");
}

#[test]
fn fixed_buf_cuts_silently() {
    let mut b: FixedBuf<8> = FixedBuf::new();
    let _ = write!(b, "{}-{}", String::from("abcdef"), String::from("ghijkl"));
    assert_eq!(b.as_bytes(), b"abcdef-g");
}

#[test]
fn push_flat_removes_newlines_and_cuts() {
    let mut b: FixedBuf<10> = FixedBuf::new();
    b.push_flat(b"a\nb\tc");
    b.push_flat(b"\r\n12345678");
    assert_eq!(b.as_bytes(), b"a b c  123");
    assert_eq!(b.as_bytes().len(), 10);
}

#[test]
fn ticks_convert() {
    assert_eq!(ticks_to_ms(250, 250), 1000);
    assert_eq!(ticks_to_ms(1, 250), 4);
    assert_eq!(ticks_to_ms(u64::MAX, 250), u32::MAX);
}

#[test]
fn line_assembler_splits_and_cuts() {
    let mut a = LineAsm::new();
    let mut got: Vec<Vec<u8>> = Vec::new();
    a.feed(b"hello\r\nwor", |l| got.push(l.to_vec()));
    a.feed(b"ld\n\n\nx", |l| got.push(l.to_vec()));
    assert_eq!(got, vec![b"hello".to_vec(), b"world".to_vec()]);
    let mut long = Vec::new();
    a.feed(&[b'y'; 450], |l| long.push(l.len()));
    assert_eq!(long, vec![MAX_MSG, MAX_MSG]);
}

#[test]
fn classify_lines() {
    assert_eq!(classify(b"[trace] frame stats"), None);
    assert_eq!(classify(b"TSC calibrated: 1000 kHz"), Some(Level::Info));
    assert_eq!(classify(b"thread 'fetcher' died: x"), Some(Level::Error));
    assert_eq!(classify(b"KERNEL PANIC: boom"), Some(Level::Fatal));
    assert_eq!(classify(b"FATAL EXCEPTION #GP"), Some(Level::Fatal));
    assert_eq!(
        classify(b"net: static fallback (no DHCP offer)"),
        Some(Level::Warn)
    );
    assert_eq!(
        classify(b"OJFS: disk read failed; RAM-only"),
        Some(Level::Warn)
    );
    assert_eq!(classify(b"first desktop frame"), Some(Level::Info));
}

#[test]
fn tail_keeps_whole_lines() {
    let d = b"aaaa\nbbbb\ncccc\n";
    assert_eq!(tail_lines(d, 100), (&d[..], false));
    // The cut lands exactly on a line start: that line is kept.
    assert_eq!(tail_lines(d, 10), (&b"bbbb\ncccc\n"[..], true));
    // The cut lands inside "bbbb": skip to the next line.
    assert_eq!(tail_lines(d, 9), (&b"cccc\n"[..], true));
    assert_eq!(tail_lines(d, 5), (&b"cccc\n"[..], true));
    assert_eq!(tail_lines(d, 4), (&b""[..], true));
    assert_eq!(tail_lines(b"no newline at all", 4), (&b""[..], true));
}

#[test]
fn view_top_for_and_set_top() {
    let mut r = Ring::new();
    for i in 0..20u32 {
        r.push(i, Level::Info, 0, b"line");
    }
    let s = snap(&r);
    let mut v = LogView::new();
    v.rebuild(&s, &Filter::new(), 5);
    assert_eq!(v.top_for(5), 15);
    // A taller window (more rows) while following: still the newest page.
    assert_eq!(v.top_for(10), 10);
    assert_eq!(v.visible(&s, 10).count(), 10);
    v.set_top(3, 5);
    assert!(!v.follow);
    assert_eq!(v.top_for(5), 3);
    assert_eq!(v.top_for(19), 1); // clamped for a very tall window
    v.set_top(999, 5);
    assert!(v.follow);
    assert_eq!(v.top_for(5), 15);
}

#[test]
fn record_at_rejects_truncated_data() {
    let mut r = Ring::new();
    r.push(0, Level::Info, 0, b"abcdef");
    let s = snap(&r);
    assert!(record_at(&s[..s.len() - 1], 0).is_none());
    assert!(record_at(&s, usize::MAX).is_none());
    assert!(record_at(&[], 0).is_none());
}
