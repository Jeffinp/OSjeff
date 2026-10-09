use super::*;

#[test]
fn declared_size_wins() {
    assert_eq!(img_box(100, 50, ImgState::Pending, 600, 100), (100, 50));
    assert_eq!(
        img_box(100, 50, ImgState::Ready { w: 999, h: 999 }, 600, 100),
        (100, 50)
    );
}

#[test]
fn width_only_uses_the_aspect_ratio_when_known() {
    assert_eq!(
        img_box(100, 0, ImgState::Ready { w: 400, h: 200 }, 600, 100),
        (100, 50)
    );
}

#[test]
fn height_only_uses_the_aspect_ratio_when_known() {
    assert_eq!(
        img_box(0, 50, ImgState::Ready { w: 400, h: 200 }, 600, 100),
        (100, 50)
    );
}

#[test]
fn pending_with_one_side_guesses_4_to_3() {
    assert_eq!(img_box(200, 0, ImgState::Pending, 600, 100), (200, 150));
    assert_eq!(img_box(0, 90, ImgState::Pending, 600, 100), (120, 90));
}

#[test]
fn no_attributes_uses_the_natural_size() {
    assert_eq!(
        img_box(0, 0, ImgState::Ready { w: 320, h: 240 }, 600, 100),
        (320, 240)
    );
}

#[test]
fn no_attributes_pending_is_a_default_box() {
    assert_eq!(img_box(0, 0, ImgState::Pending, 600, 100), (160, 120));
}

#[test]
fn a_box_wider_than_the_line_shrinks_keeping_the_ratio() {
    assert_eq!(
        img_box(0, 0, ImgState::Ready { w: 1000, h: 500 }, 250, 100),
        (250, 125)
    );
    assert_eq!(img_box(800, 400, ImgState::Pending, 200, 100), (200, 100));
}

#[test]
fn zoom_scales_the_natural_size() {
    assert_eq!(
        img_box(0, 0, ImgState::Ready { w: 100, h: 60 }, 900, 200),
        (200, 120)
    );
    assert_eq!(
        img_box(0, 0, ImgState::Ready { w: 100, h: 60 }, 900, 50),
        (50, 30)
    );
}

#[test]
fn failed_states_get_a_message_box() {
    for s in [
        ImgState::Unsupported,
        ImgState::Failed,
        ImgState::TooBig,
        ImgState::TooMany,
    ] {
        assert_eq!(img_box(0, 0, s, 600, 100), (240, 48));
    }
}

#[test]
fn sizes_are_never_zero_or_huge() {
    for (dw, dh) in [(0, 0), (1, 0), (0, 1), (4096, 4096)] {
        for s in [
            ImgState::Pending,
            ImgState::Ready { w: 1, h: 100000 },
            ImgState::Ready { w: 100000, h: 1 },
        ] {
            let (w, h) = img_box(dw, dh, s, 700, 300);
            assert!(
                (1..=20_000).contains(&w) && (1..=20_000).contains(&h),
                "{dw} {dh} {s:?} -> {w}x{h}"
            );
        }
    }
}
