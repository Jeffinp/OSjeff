use super::*;

#[test]
fn cpuid_bit_30_means_rdrand() {
    assert!(has_rdrand(1 << 30));
    assert!(has_rdrand(0xFFFF_FFFF));
    assert!(!has_rdrand(0));
    assert!(!has_rdrand(!(1u32 << 30)));
    assert_eq!(CPUID_ECX_RDRAND, 0x4000_0000);
}

#[test]
fn cpuid_leaf7_bit_18_means_rdseed_only_if_the_leaf_exists() {
    assert!(has_rdseed(7, 1 << 18));
    assert!(has_rdseed(0x16, 0xFFFF_FFFF));
    assert!(!has_rdseed(7, 0));
    assert!(!has_rdseed(7, !(1u32 << 18)));
    // A CPU with no leaf 7 returns the data of its highest leaf: ignore it.
    assert!(!has_rdseed(6, 0xFFFF_FFFF));
    assert!(!has_rdseed(1, 1 << 18));
    assert_eq!(CPUID_EBX7_RDSEED, 0x0004_0000);
}

#[test]
fn retry_hw_returns_first_success() {
    let mut calls = 0;
    let v = retry_hw(|| {
        calls += 1;
        Some(42)
    });
    assert_eq!(v, Some(42));
    assert_eq!(calls, 1);
}

#[test]
fn retry_hw_retries_up_to_the_limit_then_gives_up() {
    // Succeeds on the last allowed attempt.
    let mut calls = 0;
    let v = retry_hw(|| {
        calls += 1;
        (calls == RDRAND_RETRIES).then_some(7)
    });
    assert_eq!(v, Some(7));
    assert_eq!(calls, RDRAND_RETRIES);
    // One attempt too late: gives up after exactly RDRAND_RETRIES tries.
    let mut calls = 0;
    let v = retry_hw(|| {
        calls += 1;
        (calls == RDRAND_RETRIES + 1).then_some(7)
    });
    assert_eq!(v, None);
    assert_eq!(calls, RDRAND_RETRIES);
    // A hardware that never succeeds is reported as failure, not as 0.
    assert_eq!(retry_hw(|| None), None);
}

#[test]
fn retry_n_uses_the_given_budget() {
    let mut calls = 0;
    assert_eq!(
        retry_n(RDSEED_RETRIES, || {
            calls += 1;
            None
        }),
        None
    );
    assert_eq!(calls, RDSEED_RETRIES);
    assert_eq!(retry_n(0, || Some(1)), None);
    const { assert!(RDSEED_RETRIES > RDRAND_RETRIES) };
}

#[test]
fn known_bad_words_are_rejected() {
    assert!(known_bad_word(0));
    assert!(known_bad_word(u64::MAX));
    assert!(known_bad_word(0xFFFF_FFFF)); // the AMD bug, 32-bit form
    assert!(known_bad_word(0xFFFF_FFFF_0000_0000));
    assert!(!known_bad_word(1));
    assert!(!known_bad_word(0x0123_4567_89AB_CDEF));
}

#[test]
fn plausible_block_rejects_stuck_and_broken_output() {
    let good = [
        0x0123_4567_89AB_CDEF,
        0xFEDC_BA98_7654_3210,
        0x1357_9BDF_0246_8ACE,
        0xDEAD_BEEF_CAFE_F00D,
    ];
    assert!(plausible_block(&good));
    assert!(!plausible_block(&[0; 4]));
    assert!(!plausible_block(&[u64::MAX; 4]));
    assert!(!plausible_block(&[0x1234_5678_9ABC_DEF0; 4])); // stuck on one value
    assert!(!plausible_block(&[1, 2, 0, 4])); // one known-bad word poisons the block
    assert!(!plausible_block(&[0xFFFF_FFFF, 5]));
    // A single word cannot show "stuck", only "known bad".
    assert!(plausible_block(&[0x1234_5678_9ABC_DEF0]));
    assert!(!plausible_block(&[0]));
    assert!(plausible_block(&[]));
}
