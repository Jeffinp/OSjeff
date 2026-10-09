//! One test per PNG vector (generated list), plus a header check.
use super::vectors::*;
use super::*;
use crate::testutil::unhex;

type Vector = (&'static str, usize, usize, &'static str);

fn check(v: Vector) {
    let (file, w, h, expect) = v;
    let bytes = unhex(file);
    let hdr = read_header(&bytes).unwrap();
    assert_eq!((hdr.width as usize, hdr.height as usize), (w, h));
    let img = decode(&bytes).unwrap();
    assert_eq!((img.width(), img.height()), (w, h));
    let got = img.to_rgba().unwrap();
    let want = unhex(expect);
    if got != want {
        let i = got.iter().zip(&want).position(|(a, b)| a != b).unwrap();
        panic!(
            "first mismatch at pixel {} channel {}: got {:?} want {:?}",
            i / 4,
            i % 4,
            &got[i / 4 * 4..i / 4 * 4 + 4],
            &want[i / 4 * 4..i / 4 * 4 + 4]
        );
    }
}

macro_rules! vector_test {
    ($name:ident, $v:expr) => {
        #[test]
        fn $name() {
            check($v);
        }
    };
}

vector_test!(im_rgb8, IM_RGB8);
vector_test!(im_rgba8, IM_RGBA8);
vector_test!(im_rgb16, IM_RGB16);
vector_test!(im_rgba16, IM_RGBA16);
vector_test!(im_pal8_trns, IM_PAL8_TRNS);
vector_test!(im_pal4, IM_PAL4);
vector_test!(im_pal4_b, IM_PAL4_B);
vector_test!(im_pal2_b, IM_PAL2_B);
vector_test!(im_gray8, IM_GRAY8);
vector_test!(im_gray16, IM_GRAY16);
vector_test!(im_gray1, IM_GRAY1);
vector_test!(im_gray2, IM_GRAY2);
vector_test!(im_gray4, IM_GRAY4);
vector_test!(im_ga8, IM_GA8);
vector_test!(im_ga16, IM_GA16);
vector_test!(im_rgb8_adam7, IM_RGB8_ADAM7);
vector_test!(im_rgba8_adam7, IM_RGBA8_ADAM7);
vector_test!(im_rgb16_adam7, IM_RGB16_ADAM7);
vector_test!(im_pal4_adam7, IM_PAL4_ADAM7);
vector_test!(im_gray1_adam7, IM_GRAY1_ADAM7);
vector_test!(im_ga8_adam7, IM_GA8_ADAM7);
vector_test!(im_rgba8_1x1_adam7, IM_RGBA8_1X1_ADAM7);
vector_test!(im_rgb8_3x2_adam7, IM_RGB8_3X2_ADAM7);
vector_test!(im_rgb8_2x7_adam7, IM_RGB8_2X7_ADAM7);
vector_test!(im_rgba8_1x9, IM_RGBA8_1X9);
vector_test!(im_rgba8_9x1, IM_RGBA8_9X1);
vector_test!(cr_rgb8_filters, CR_RGB8_FILTERS);
vector_test!(cr_rgba8_filters, CR_RGBA8_FILTERS);
vector_test!(cr_gray8_filters, CR_GRAY8_FILTERS);
vector_test!(cr_ga8_filters, CR_GA8_FILTERS);
vector_test!(cr_rgb16_filters, CR_RGB16_FILTERS);
vector_test!(cr_rgba16_filters, CR_RGBA16_FILTERS);
vector_test!(cr_ga16_filters, CR_GA16_FILTERS);
vector_test!(cr_gray16_filters, CR_GRAY16_FILTERS);
vector_test!(cr_pal8_filters, CR_PAL8_FILTERS);
vector_test!(cr_pal4_filters, CR_PAL4_FILTERS);
vector_test!(cr_pal2_filters, CR_PAL2_FILTERS);
vector_test!(cr_pal1_filters, CR_PAL1_FILTERS);
vector_test!(cr_gray4_filters, CR_GRAY4_FILTERS);
vector_test!(cr_gray2_filters, CR_GRAY2_FILTERS);
vector_test!(cr_gray1_filters, CR_GRAY1_FILTERS);
vector_test!(cr_gray8_key, CR_GRAY8_KEY);
vector_test!(cr_gray2_key, CR_GRAY2_KEY);
vector_test!(cr_gray16_key, CR_GRAY16_KEY);
vector_test!(cr_rgb8_key, CR_RGB8_KEY);
vector_test!(cr_rgb16_key, CR_RGB16_KEY);
vector_test!(cr_pal8_trns_short, CR_PAL8_TRNS_SHORT);
vector_test!(cr_pal4_trns_full, CR_PAL4_TRNS_FULL);
vector_test!(cr_rgba8_adam7_filters, CR_RGBA8_ADAM7_FILTERS);
vector_test!(cr_rgb16_adam7_filters, CR_RGB16_ADAM7_FILTERS);
vector_test!(cr_pal2_adam7_filters, CR_PAL2_ADAM7_FILTERS);
vector_test!(cr_gray1_adam7_filters, CR_GRAY1_ADAM7_FILTERS);
vector_test!(cr_gray4_adam7_key, CR_GRAY4_ADAM7_KEY);
vector_test!(cr_rgba8_multi_idat, CR_RGBA8_MULTI_IDAT);
vector_test!(cr_rgb8_ancillary, CR_RGB8_ANCILLARY);
vector_test!(cr_rgb8_level0, CR_RGB8_LEVEL0);
