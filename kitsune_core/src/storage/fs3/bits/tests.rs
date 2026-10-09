use super::*;

#[test]
fn padding_bits_are_never_free() {
    let v = new(70);
    assert_eq!(v.len(), 2);
    assert_eq!(count_free(&v, 70), 70);
    assert_eq!(next_free(&v, 70, 128), None);
    assert!(get(&v, 70) && get(&v, 127));
    assert!(!get(&v, 69));
}

#[test]
fn set_clear_get_count() {
    let mut v = new(200);
    for i in [0u32, 1, 63, 64, 65, 127, 199] {
        set(&mut v, i);
        assert!(get(&v, i));
    }
    assert_eq!(count_free(&v, 200), 193);
    clear(&mut v, 64);
    assert!(!get(&v, 64));
    assert_eq!(count_free(&v, 200), 194);
    assert!(!get(&v, 1000)); // out of range reads as 0, never panics
    set(&mut v, 100_000); // out of range writes are ignored
}

#[test]
fn next_free_and_used_cross_word_boundaries() {
    let mut v = new(300);
    for i in 0..150 {
        set(&mut v, i);
    }
    assert_eq!(next_free(&v, 0, 300), Some(150));
    assert_eq!(next_free(&v, 151, 300), Some(151));
    assert_eq!(next_used(&v, 150, 300), None);
    assert_eq!(next_used(&v, 0, 300), Some(0));
    assert_eq!(next_used(&v, 100, 120), Some(100));
    assert_eq!(next_free(&v, 0, 150), None);
    assert_eq!(next_free(&v, 299, 300), Some(299));
    assert_eq!(next_free(&v, 300, 300), None);
}

#[test]
fn find_run_finds_first_big_enough_gap() {
    let mut v = new(256);
    // used: 0..10, 12..20, 25..256 -> gaps of 2 (10..12), 5 (20..25)
    for i in (0..10).chain(12..20).chain(25..256) {
        set(&mut v, i);
    }
    assert_eq!(find_run(&v, 0, 256, 1), Some(10));
    assert_eq!(find_run(&v, 0, 256, 2), Some(10));
    assert_eq!(find_run(&v, 0, 256, 3), Some(20));
    assert_eq!(find_run(&v, 0, 256, 5), Some(20));
    assert_eq!(find_run(&v, 0, 256, 6), None);
    assert_eq!(find_run(&v, 11, 256, 2), Some(20));
}

#[test]
fn find_run_respects_the_upper_bound() {
    let v = new(256);
    assert_eq!(find_run(&v, 0, 100, 100), Some(0));
    assert_eq!(find_run(&v, 0, 100, 101), None);
    assert_eq!(find_run(&v, 90, 100, 11), None);
}

#[test]
fn zero_bits_vector_is_harmless() {
    let v = new(0);
    assert!(v.is_empty());
    assert_eq!(count_free(&v, 0), 0);
    assert_eq!(next_free(&v, 0, 0), None);
}
