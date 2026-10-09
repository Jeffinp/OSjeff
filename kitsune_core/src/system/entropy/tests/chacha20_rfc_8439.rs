use super::*;

#[test]
fn rfc8439_quarter_round() {
    // Section 2.1.1.
    let (a, b, c, d) = chacha::quarter_round(0x1111_1111, 0x0102_0304, 0x9b8d_6f43, 0x0123_4567);
    assert_eq!(
        (a, b, c, d),
        (0xea2a_92f4, 0xcb1c_f8ce, 0x4581_472e, 0x5881_c4bb)
    );
}

#[test]
fn rfc8439_block_function() {
    // Section 2.3.2: key 00..1f, counter 1, nonce 00:00:00:09:00:00:00:4a:00:00:00:00.
    let nonce = [0, 0, 0, 9, 0, 0, 0, 0x4a, 0, 0, 0, 0];
    let out = chacha::block(&key_00_1f(), 1, &nonce);
    let want = hex(
        "10f1e7e4d13b5915500fdd1fa32071c4c7d1f4c733c068030422aa9ac3d46c4e\
         d2826446079faa0914c2d705d98b02a2b5129cd1de164eb9cbd083e8a2503c4e",
    );
    assert_eq!(&out[..], &want[..]);
}

#[test]
fn rfc8439_encryption_example() {
    // Section 2.4.2: the "sunscreen" plaintext, counter 1.
    let pt = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.";
    let nonce = [0, 0, 0, 0, 0, 0, 0, 0x4a, 0, 0, 0, 0];
    let mut ks = alloc::vec![0u8; pt.len()];
    assert_eq!(
        chacha::keystream(&key_00_1f(), &nonce, 1, &mut ks),
        Some(1 + 2)
    );
    let ct: Vec<u8> = pt.iter().zip(&ks).map(|(p, k)| p ^ k).collect();
    let want = hex(
        "6e2e359a2568f98041ba0728dd0d6981e97e7aec1d4360c20a27afccfd9fae0b\
         f91b65c5524733ab8f593dabcd62b3571639d624e65152ab8f530c359f0861d8\
         07ca0dbf500d6a6156a38e088a22b65e52bc514d16ccf806818ce91ab7793736\
         5af90bbf74a35be6b40b8eedf2785e42874d",
    );
    assert_eq!(ct, want);
}

#[test]
fn rfc8439_appendix_a1_zero_key_vectors() {
    // Appendix A.1, tests 1 and 2: all-zero key and nonce, counters 0 and 1.
    let z = [0u8; 32];
    let n = [0u8; 12];
    assert_eq!(
        &chacha::block(&z, 0, &n)[..],
        &hex(
            "76b8e0ada0f13d90405d6ae55386bd28bdd219b8a08ded1aa836efcc8b770dc7\
             da41597c5157488d7724e03fb8d84a376a43b8f41518a11cc387b669b2ee6586"
        )[..]
    );
    assert_eq!(
        &chacha::block(&z, 1, &n)[..],
        &hex(
            "9f07e7be5551387a98ba977c732d080dcb0f29a048e3656912c6533e32ee7aed\
             29b721769ce64e43d57133b074d839d531ed1f28510afb45ace10a1f4b794d6f"
        )[..]
    );
}

#[test]
fn chacha_keystream_near_counter_wrap_matches_reference() {
    // Computed with an independent implementation (python `cryptography`).
    let nonce = hex("0102030405060708090a0b0c");
    let nonce: [u8; 12] = nonce.try_into().unwrap();
    let mut out = [0u8; 128];
    // Blocks 0xfffffffe and 0xffffffff are both usable; the counter space is then exhausted.
    assert_eq!(
        chacha::keystream(&key_00_1f(), &nonce, 0xffff_fffe, &mut out),
        None
    );
    let want = hex(
        "0a6ef904698cfed0e2a9d264cf24029803976e6dfe192f36866801bae8dcd28d\
         1b9f7d81ccad0bc769f1f3af36276e3e47e0cf943c2b3b0022363525db15520b\
         c4e9bc95855bfc7f5456e79b23564422a662e195208fc8a7d24bd6c35a366573\
         688febc54eade68bc3d3e3529dcec58ac73440107ff9021defca771ad3825c3d",
    );
    assert_eq!(&out[..], &want[..]);
}

#[test]
fn chacha_keystream_prefix_property() {
    // Any prefix of a longer request is the same keystream.
    let n = [7u8; 12];
    let mut long = [0u8; 3 * BLOCK_LEN + 5];
    let mut short = [0u8; BLOCK_LEN + 9];
    chacha::keystream(&key_00_1f(), &n, 3, &mut long).unwrap();
    chacha::keystream(&key_00_1f(), &n, 3, &mut short).unwrap();
    assert_eq!(&long[..short.len()], &short[..]);
}
