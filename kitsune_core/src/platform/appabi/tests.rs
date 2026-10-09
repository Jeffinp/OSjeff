use super::*;

#[test]
fn range_inside() {
    assert_eq!(check_range(100, 0, 100), Ok(0..100));
    assert_eq!(check_range(100, 10, 20), Ok(10..30));
    assert_eq!(check_range(100, 100, 0), Ok(100..100));
    assert_eq!(check_range(0, 0, 0), Ok(0..0));
}

#[test]
fn range_outside() {
    assert_eq!(check_range(100, 0, 101), Err(Fault));
    assert_eq!(check_range(100, 101, 0), Err(Fault));
    assert_eq!(check_range(100, 99, 2), Err(Fault));
    assert_eq!(check_range(0, 0, 1), Err(Fault));
}

#[test]
fn negative_values_are_huge_not_small() {
    assert_eq!(check_range(1 << 20, -1, 1), Err(Fault));
    assert_eq!(check_range(1 << 20, 0, -1), Err(Fault));
    assert!(check_range(usize::MAX, i32::MIN, 0).is_ok());
    assert_eq!(check_range(100, i32::MIN, 1), Err(Fault));
}

#[test]
fn no_overflow_at_the_extremes() {
    assert_eq!(
        check_range(usize::MAX, -1, -1),
        Ok(u32::MAX as usize..(u32::MAX as u64 * 2) as usize)
    );
    assert_eq!(check_range(10, i32::MAX, i32::MAX), Err(Fault));
}

#[test]
fn capped_variants() {
    assert_eq!(check_capped(100, 0, 10, 16), Ok(Ok(0..10)));
    assert_eq!(check_capped(100, 0, 17, 16), Ok(Err(ERR_INVAL)));
    assert_eq!(check_capped(100, 200, 17, 16), Err(Fault));
    assert_eq!(check_capped(100, 0, -5, 16), Err(Fault));
    assert_eq!(check_capped(100, 95, 10, 16), Err(Fault));
}

#[test]
fn error_codes_are_distinct_and_negative() {
    let all = [
        ERR_PERM,
        ERR_NOENT,
        ERR_BADF,
        ERR_INVAL,
        ERR_EXIST,
        ERR_NOSPC,
        ERR_MFILE,
        ERR_NOTDIR,
        ERR_ISDIR,
        ERR_NOTEMPTY,
        ERR_NET,
        ERR_NOSYS,
    ];
    for (i, a) in all.iter().enumerate() {
        assert!(*a < 0);
        for b in &all[i + 1..] {
            assert_ne!(a, b);
        }
    }
}

#[test]
fn open_flags_are_disjoint_bits() {
    assert_eq!(O_READ | O_WRITE | O_CREATE | O_TRUNC | O_APPEND, O_ALL);
    assert_eq!(O_ALL, 31);
}
