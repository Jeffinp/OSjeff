use super::*;

#[test]
fn rgba_and_channels_roundtrip() {
    assert_eq!(rgba(1, 2, 3, 4), 0x0401_0203);
    assert_eq!(channels(0x0401_0203), [1, 2, 3, 4]);
    assert_eq!(alpha(0x8000_0000), 0x80);
    for v in [0u32, 0xFFFF_FFFF, 0x1234_5678, 0x8000_0001] {
        let [r, g, b, a] = channels(v);
        assert_eq!(rgba(r, g, b, a), v);
    }
}

#[test]
fn new_image_is_filled() {
    let i = Image::new(3, 2, 0xAABB_CCDD).unwrap();
    assert_eq!((i.width(), i.height()), (3, 2));
    assert_eq!(i.pixels().len(), 6);
    assert!(i.pixels().iter().all(|&p| p == 0xAABB_CCDD));
    assert!(format!("{i:?}").contains("3x2"));
}

#[test]
fn zero_sizes_are_rejected() {
    assert_eq!(Image::new(0, 5, 0), Err(ImageError::ZeroSize));
    assert_eq!(Image::new(5, 0, 0), Err(ImageError::ZeroSize));
    assert_eq!(pixel_count(0, 0), Err(ImageError::ZeroSize));
}

#[test]
fn absurd_dimensions_are_rejected_without_allocating() {
    // These would need exabytes; the check must fire first (the test would
    // abort on allocation failure otherwise).
    assert_eq!(
        Image::new(usize::MAX, usize::MAX, 0),
        Err(ImageError::TooLarge)
    );
    assert_eq!(Image::new(usize::MAX, 2, 0), Err(ImageError::TooLarge));
    assert_eq!(Image::new(1 << 40, 1 << 40, 0), Err(ImageError::TooLarge));
    assert_eq!(Image::new(100_000, 100_000, 0), Err(ImageError::TooLarge));
    assert_eq!(pixel_count(65_536, 65_536), Err(ImageError::TooLarge));
}

#[test]
fn pixel_limit_is_exact() {
    assert_eq!(pixel_count(MAX_PIXELS, 1), Ok(MAX_PIXELS));
    assert_eq!(pixel_count(1, MAX_PIXELS), Ok(MAX_PIXELS));
    assert_eq!(pixel_count(4096, 4096), Ok(MAX_PIXELS));
    assert_eq!(pixel_count(MAX_PIXELS + 1, 1), Err(ImageError::TooLarge));
    assert_eq!(pixel_count(4097, 4096), Err(ImageError::TooLarge));
    assert_eq!(MAX_PIXELS, 16 * 1024 * 1024);
}

#[test]
fn from_pixels_checks_the_buffer_length() {
    assert_eq!(
        Image::from_pixels(2, 2, vec![0; 3]),
        Err(ImageError::BadBuffer)
    );
    assert_eq!(
        Image::from_pixels(2, 2, vec![0; 5]),
        Err(ImageError::BadBuffer)
    );
    assert!(Image::from_pixels(2, 2, vec![0; 4]).is_ok());
    assert_eq!(
        Image::from_pixels(usize::MAX, 3, vec![]),
        Err(ImageError::TooLarge)
    );
}

#[test]
fn rgba_bytes_roundtrip_and_validate() {
    let bytes = [1, 2, 3, 4, 5, 6, 7, 8];
    let i = Image::from_rgba(2, 1, &bytes).unwrap();
    assert_eq!(i.pixels(), &[rgba(1, 2, 3, 4), rgba(5, 6, 7, 8)]);
    assert_eq!(i.to_rgba().unwrap(), bytes);
    assert_eq!(
        Image::from_rgba(2, 1, &bytes[..7]),
        Err(ImageError::BadBuffer)
    );
    assert_eq!(Image::from_rgba(0, 1, &[]), Err(ImageError::ZeroSize));
}

#[test]
fn get_set_and_rows_are_bounds_safe() {
    let mut i = Image::new(3, 2, 0).unwrap();
    assert!(i.set(2, 1, 7));
    assert_eq!(i.get(2, 1), Some(7));
    assert_eq!(i.get(3, 0), None);
    assert_eq!(i.get(0, 2), None);
    assert!(!i.set(3, 0, 1));
    assert!(!i.set(0, 2, 1));
    assert!(!i.set(usize::MAX, usize::MAX, 1));
    assert_eq!(i.row(1), &[0, 0, 7]);
    assert!(i.row(2).is_empty());
    assert!(i.row(usize::MAX).is_empty());
    assert!(i.row_mut(usize::MAX).is_empty());
    i.row_mut(0)[1] = 9;
    assert_eq!(i.pixels(), &[0, 9, 0, 0, 0, 7]);
    assert_eq!(i.clone().into_pixels().len(), 6);
    i.pixels_mut()[0] = 1;
    assert_eq!(i.get(0, 0), Some(1));
}

#[test]
fn opacity_detection() {
    assert!(Image::new(2, 2, BLACK).unwrap().is_opaque());
    let mut i = Image::new(2, 2, BLACK).unwrap();
    i.pixels_mut()[3] = 0xFE00_0000;
    assert!(!i.is_opaque());
}
