use super::*;
use crate::i18n::Lang;
use crate::i18n::testlang::LangGuard;

fn s(b: FixedBuf<20>) -> String {
    String::from_utf8_lossy(b.as_bytes()).into_owned()
}

#[test]
fn names_are_translated() {
    let _lang = LangGuard::new(Lang::Pt);
    assert_eq!(friendly_name(b"compositor"), "Interface");
    assert_eq!(friendly_name(b"fetcher"), "Rede (busca)");
    assert_eq!(friendly_name(b"appd"), "Aplicativos");
    assert_eq!(friendly_name(b"shelld"), "Terminal (execução)");
    assert_eq!(friendly_name(b"shelld2"), "Terminal (execução)");
    assert_eq!(friendly_name(b"logd"), "Registro");
    assert_eq!(friendly_name(b"kernel"), "Sistema");
    assert_eq!(friendly_name(b"shell"), "Terminal");
    assert_eq!(friendly_name(b"shell 2"), "Terminal 2");
    assert_eq!(friendly_name(b"calc 12"), "Calculadora 12");
    assert_eq!(friendly_name(b"mystery"), "mystery");
    assert_eq!(friendly_name(b"mystery 7"), "mystery 7");
    assert_eq!(friendly_name(b""), "");
    // A long numeric tail is not an instance number.
    assert_eq!(friendly_name(b"app 12345"), "app 12345");
}

#[test]
fn english_names_numbers_and_labels() {
    let _lang = LangGuard::new(Lang::En);
    assert_eq!(friendly_name(b"fetcher"), "Network (fetch)");
    assert_eq!(friendly_name(b"shell 2"), "Terminal 2");
    assert_eq!(friendly_name(b"taskmgr"), "Tasks");
    assert_eq!(friendly_name(b"(idle)"), "Idle");
    assert_eq!(fmt_pct(123).as_bytes(), b"12.3%");
    assert_eq!(s(fmt_size(1536)), "1.5 KiB");
    assert_eq!(fmt_speed(2048).as_bytes(), b"2.0 KiB/s");
    assert_eq!(fmt_count(1234567).as_bytes(), b"1,234,567");
    assert_eq!(s(fmt_elapsed(185)), "3 min 05 s");
    assert_eq!(fmt_ago(0).as_bytes(), b"now");
    assert_eq!(fmt_ago(12).as_bytes(), b"12 s ago");
    assert_eq!(fmt_milli(420).as_bytes(), b"0.42");
    assert_eq!(fmt_log_time(205_100).as_bytes(), b"3:25.100");
    assert_eq!(level_name(crate::system::klog::Level::Warn), "WARN");
    assert_eq!(level_name(crate::system::klog::Level::Error), "ERROR");
    assert_eq!(Pressure::Attention.label(), "Elevated");
    assert_eq!(TaskState::Waiting.label(), "Waiting");
    assert_eq!(Column::Up.title(), "Uptime");
    let _pt = LangGuard::new(Lang::Pt);
    assert_eq!(level_name(crate::system::klog::Level::Warn), "AVISO");
    assert_eq!(Column::Name.title(), "Nome");
}

#[test]
fn system_names() {
    assert!(is_system_name(b"appd"));
    assert!(is_system_name(b"kernel"));
    assert!(!is_system_name(b"shell"));
    assert!(!is_system_name(b"calc 2"));
}

#[test]
fn pt_formatting() {
    let _lang = LangGuard::new(Lang::Pt);
    assert_eq!(fmt_pct(0).as_bytes(), b"0,0%");
    assert_eq!(fmt_pct(123).as_bytes(), b"12,3%");
    assert_eq!(fmt_pct(1000).as_bytes(), b"100,0%");
    assert_eq!(fmt_pct_int(125).as_bytes(), b"13%");
    assert_eq!(fmt_pct_int(4).as_bytes(), b"0%");
    assert_eq!(s(fmt_size(0)), "0 B");
    assert_eq!(s(fmt_size(1023)), "1023 B");
    assert_eq!(s(fmt_size(1024)), "1,0 KiB");
    assert_eq!(s(fmt_size(1536)), "1,5 KiB");
    assert_eq!(s(fmt_size(64 << 20)), "64,0 MiB");
    assert_eq!(s(fmt_size(3 << 30)), "3,0 GiB");
    assert_eq!(fmt_speed(2048).as_bytes(), b"2,0 KiB/s");
    assert_eq!(fmt_count(0).as_bytes(), b"0");
    assert_eq!(fmt_count(999).as_bytes(), b"999");
    assert_eq!(fmt_count(1000).as_bytes(), b"1.000");
    assert_eq!(fmt_count(1234567).as_bytes(), b"1.234.567");
    assert_eq!(s(fmt_elapsed(7)), "7 s");
    assert_eq!(s(fmt_elapsed(185)), "3 min 05 s");
    assert_eq!(s(fmt_elapsed(3725)), "1 h 02 min");
    assert_eq!(s(fmt_elapsed(90_061)), "1 d 01 h");
    assert_eq!(s(fmt_clock(3725)), "01:02:05");
    assert_eq!(s(fmt_clock(90_061)), "1 d 01:01:01");
    assert_eq!(fmt_ago(0).as_bytes(), b"agora");
    assert_eq!(fmt_ago(12).as_bytes(), "há 12 s".as_bytes());
    assert_eq!(fmt_milli(420).as_bytes(), b"0,42");
    assert_eq!(fmt_log_time(0).as_bytes(), b"0,000");
    assert_eq!(fmt_log_time(12_345).as_bytes(), b"12,345");
    assert_eq!(fmt_log_time(205_100).as_bytes(), b"3:25,100");
    assert_eq!(fmt_log_time(3_723_400).as_bytes(), b"1:02:03,400");
    let _ = fmt_log_time(u32::MAX);
    assert_eq!(level_name(crate::system::klog::Level::Warn), "AVISO");
    assert_eq!(level_name(crate::system::klog::Level::Fatal), "FATAL");
    assert_eq!(fmt_milli(1000).as_bytes(), b"1,00");
    // Extremes never panic.
    let _ = fmt_size(u64::MAX);
    let _ = fmt_elapsed(u64::MAX);
    let _ = fmt_count(u64::MAX);
    let _ = fmt_pct(u32::MAX);
}

#[test]
fn easing_settles_and_never_overshoots() {
    let mut v = 0;
    let target = 100 << 8;
    let mut steps = 0;
    while v != target {
        let n = ease_toward(v, target, 16, 120);
        assert!(n > v && n <= target);
        v = n;
        steps += 1;
        assert!(steps < 2000, "must settle");
    }
    // Downwards too.
    let mut v = 100 << 8;
    while v != 0 {
        let n = ease_toward(v, 0, 16, 120);
        assert!((0..v).contains(&n));
        v = n;
    }
    // A huge dt lands (nearly) on the target in one step, never beyond it.
    assert!(ease_toward(0, 256, 10_000, 100) <= 256);
    assert_eq!(ease_toward(5, 5, 16, 100), 5);
}

#[test]
fn glide_moves_then_rests() {
    let mut g = Glide::at(10);
    assert!(!g.moving());
    g.set(50);
    assert!(g.moving());
    let mut guard = 0;
    while g.step(16, 80) {
        guard += 1;
        assert!(guard < 1000);
    }
    assert_eq!(g.value(), 50);
    g.set(0);
    g.snap();
    assert_eq!(g.value(), 0);
    assert!(!g.moving());
}

#[test]
fn series_interpolates() {
    let mut ser = Series::new();
    assert_eq!(series_at(&ser, 0), None);
    for v in [0u32, 100, 200] {
        ser.push(v);
    }
    assert_eq!(series_at(&ser, 0), Some(0));
    assert_eq!(series_at(&ser, 128), Some(50));
    assert_eq!(series_at(&ser, 256), Some(100));
    assert_eq!(series_at(&ser, 384), Some(150));
    assert_eq!(series_at(&ser, 512), Some(200));
    // Clamped at both ends.
    assert_eq!(series_at(&ser, -500), Some(0));
    assert_eq!(series_at(&ser, 9999), Some(200));
}

#[test]
fn smoothing_keeps_a_flat_line_and_rounds_a_spike() {
    let mut v = [40u32, 40, 40, 40];
    smooth121(&mut v);
    assert_eq!(v, [40, 40, 40, 40]);
    let mut v = [0u32, 0, 100, 0, 0];
    smooth121(&mut v);
    assert_eq!(v, [0, 25, 50, 25, 0]);
    let mut v = [7u32];
    smooth121(&mut v);
    assert_eq!(v, [7]);
    let mut v: [u32; 0] = [];
    smooth121(&mut v);
    let mut v = [0u32, 100];
    smooth121(&mut v);
    assert_eq!(v, [0, 100]);
    // Large values do not overflow.
    let mut v = [u32::MAX; 4];
    smooth121(&mut v);
    assert_eq!(v, [u32::MAX; 4]);
}

#[test]
fn snapshot_copies_oldest_first() {
    let mut ser = Series::new();
    for i in 0..70u32 {
        ser.push(i);
    }
    let mut buf = [0u32; crate::system::sysmon::HIST];
    let n = snapshot(&ser, &mut buf);
    assert_eq!(n, 60);
    assert_eq!((buf[0], buf[59]), (10, 69));
    assert_eq!(slice_at(&buf[..n], 256 * 59), Some(69));
    assert_eq!(slice_at(&[], 0), None);
}

#[test]
fn pointer_to_sample() {
    // 60 slots over 590 px; a 3-sample history sits at the right edge.
    assert_eq!(sample_under(100, 100, 590, 60, 60), Some(0));
    assert_eq!(sample_under(689, 100, 590, 60, 60), Some(59));
    assert_eq!(sample_under(99, 100, 590, 60, 60), None);
    assert_eq!(sample_under(690, 100, 590, 60, 60), None);
    assert_eq!(sample_under(100, 100, 590, 60, 3), None);
    assert_eq!(sample_under(689, 100, 590, 60, 3), Some(2));
    assert_eq!(sample_under(5, 0, 0, 60, 3), None);
    assert_eq!(sample_under(5, 0, 100, 60, 0), None);
}

#[test]
fn wrapping_counters() {
    assert_eq!(wrapping_delta(10, 25, 32), 15);
    // A 32-bit counter wrapped once.
    assert_eq!(wrapping_delta(u32::MAX as u64 - 4, 5, 32), 10);
    // 64-bit, no wrap.
    assert_eq!(wrapping_delta(1, 1 << 40, 64), (1 << 40) - 1);
    // A small step backwards is a reset, not 4 billion bytes.
    assert_eq!(wrapping_delta(1000, 10, 64), 0);
    assert_eq!(wrapping_delta(0, 0, 8), 0);
    // 8-bit wrap.
    assert_eq!(wrapping_delta(250, 4, 8), 10);
}

#[test]
fn rate_from_counters() {
    let mut r = Rate::new(32);
    assert_eq!(r.feed(1000, 0), 0);
    assert_eq!(r.feed(3000, 1000), 2000);
    // Irregular interval: 1500 bytes in 500 ms.
    assert_eq!(r.feed(4500, 1500), 3000);
    // Wrap-around of a 32-bit counter.
    let mut w = Rate::new(32);
    w.feed(u32::MAX as u64 - 99, 0);
    assert_eq!(w.feed(100, 1000), 200);
    // Same timestamp: no division by zero.
    assert_eq!(w.feed(200, 1000), 0);
    // Clock going backwards.
    assert_eq!(w.feed(300, 500), 0);
}

#[test]
fn load_average_follows_and_decays() {
    let mut l = LoadAvg::new();
    assert_eq!(l.get(), (0, 0, 0));
    for _ in 0..600 {
        l.feed(500);
    }
    let (a, b, c) = l.get();
    assert!((495..=505).contains(&a), "{a}");
    assert!((490..=510).contains(&b), "{b}");
    assert!(c > 400, "{c}");
    for _ in 0..300 {
        l.feed(0);
    }
    let (a2, b2, c2) = l.get();
    assert!(a2 < 20, "{a2}");
    assert!(b2 < a && b2 > a2);
    assert!(c2 > b2);
    // Out-of-range input is clamped.
    l.feed(9999);
    assert!(l.get().0 <= 1000);
}

#[test]
fn pressure_levels() {
    let _lang = LangGuard::new(Lang::Pt);
    assert_eq!(Pressure::of(0, 0), Pressure::Normal);
    assert_eq!(Pressure::of(59, 100), Pressure::Normal);
    assert_eq!(Pressure::of(60, 100), Pressure::Attention);
    assert_eq!(Pressure::of(84, 100), Pressure::Attention);
    assert_eq!(Pressure::of(85, 100), Pressure::Critical);
    assert_eq!(Pressure::of(500, 100), Pressure::Critical);
    assert!(Pressure::Critical > Pressure::Normal);
    assert_eq!(Pressure::Attention.label(), "Atenção");
    assert_eq!(permille(1, 4), 250);
    assert_eq!(permille(9, 0), 0);
    assert_eq!(permille(9, 3), 1000);
}

fn task(id: u32, raw: &str, pid: u16, cpu: Option<u16>, mem: Option<u32>, up: u32) -> TaskRow {
    let mut r = TaskRow::new(id, raw.as_bytes(), TaskKind::App, pid, TaskState::Running);
    r.cpu_pm = cpu;
    r.mem_kib = mem;
    r.up_s = up;
    r
}

fn ids(rows: &[TaskRow]) -> Vec<u32> {
    rows.iter().map(|r| r.id).collect()
}

#[test]
fn table_sorts_every_column_both_ways() {
    let _lang = LangGuard::new(Lang::Pt);
    let mut rows = [
        task(1, "shell", 3, Some(100), None, 5),
        task(2, "editor", 2, Some(300), Some(128), 50),
        task(3, "calc", 9, None, Some(256), 20),
    ];
    sort_tasks(&mut rows, Column::Name, false);
    // Calculadora, Editor, Terminal
    assert_eq!(ids(&rows), [3, 2, 1]);
    sort_tasks(&mut rows, Column::Name, true);
    assert_eq!(ids(&rows), [1, 2, 3]);
    sort_tasks(&mut rows, Column::Pid, false);
    assert_eq!(ids(&rows), [2, 1, 3]);
    sort_tasks(&mut rows, Column::Cpu, true);
    assert_eq!(ids(&rows), [2, 1, 3]);
    sort_tasks(&mut rows, Column::Mem, true);
    assert_eq!(ids(&rows), [3, 2, 1]);
    sort_tasks(&mut rows, Column::Up, false);
    assert_eq!(ids(&rows), [1, 3, 2]);
}

#[test]
fn table_sort_is_stable_and_accent_blind() {
    let _lang = LangGuard::new(Lang::Pt);
    let mut rows = [
        task(1, "a", 1, Some(10), None, 0),
        task(2, "b", 2, Some(10), None, 0),
        task(3, "c", 3, Some(10), None, 0),
    ];
    sort_tasks(&mut rows, Column::Cpu, true);
    assert_eq!(ids(&rows), [1, 2, 3]);
    sort_tasks(&mut rows, Column::Cpu, false);
    assert_eq!(ids(&rows), [1, 2, 3]);
    // "Terminal (execução)" sorts by its folded name.
    let mut rows = [
        TaskRow::new(1, b"shelld", TaskKind::Thread, 0, TaskState::Waiting),
        TaskRow::new(2, b"appd", TaskKind::Thread, 0, TaskState::Waiting),
        TaskRow::new(3, b"compositor", TaskKind::Thread, 0, TaskState::Waiting),
    ];
    sort_tasks(&mut rows, Column::Name, false);
    assert_eq!(ids(&rows), [2, 3, 1]);
    assert_eq!(fold("Execução"), "execucao");
    assert_eq!(fold("Atenção Ônibus"), ["aten", "cao onibus"].concat());
}

#[test]
fn table_search_and_totals() {
    let _lang = LangGuard::new(Lang::Pt);
    let r = TaskRow::new(1, b"shelld", TaskKind::Thread, 0, TaskState::Waiting);
    assert!(r.matches(""));
    assert!(r.matches("execucao"));
    assert!(r.matches("shelld"));
    assert!(!r.matches("browser"));
    let rows = [
        r,
        task(2, "calc", 4, None, None, 0),
        TaskRow::new(3, b"kernel", TaskKind::System, 1, TaskState::Running),
    ];
    assert_eq!(
        totals(&rows),
        Totals {
            processes: 2,
            threads: 1
        }
    );
    assert_eq!(TaskState::Waiting.label(), "Em espera");
    assert!(Column::Cpu.default_desc() && !Column::Name.default_desc());
    assert_eq!(Column::ALL.len(), 6);
    assert_eq!(Column::Up.title(), "Tempo ativo");
}
