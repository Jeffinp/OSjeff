use super::*;

#[test]
fn crop_extracts_the_rectangle() {
    let i = img(3, 3, &[1, 2, 3, 4, 5, 6, 7, 8, 9]);
    assert_eq!(i.crop(1, 1, 2, 2).unwrap().pixels(), &[5, 6, 8, 9]);
    assert_eq!(i.crop(0, 0, 3, 3).unwrap(), i);
    assert_eq!(i.crop(2, 0, 1, 3).unwrap().pixels(), &[3, 6, 9]);
    assert_eq!(i.crop(0, 2, 3, 1).unwrap().pixels(), &[7, 8, 9]);
    let c = i.crop(1, 0, 1, 1).unwrap();
    assert_eq!((c.width(), c.height(), c.pixels()), (1, 1, &[2][..]));
}

#[test]
fn crop_rejects_bad_rectangles() {
    let i = img(3, 3, &[0; 9]);
    assert_eq!(i.crop(2, 0, 2, 1), Err(ImageError::OutOfBounds));
    assert_eq!(i.crop(0, 2, 1, 2), Err(ImageError::OutOfBounds));
    assert_eq!(i.crop(4, 0, 1, 1), Err(ImageError::OutOfBounds));
    assert_eq!(i.crop(0, 0, 0, 1), Err(ImageError::ZeroSize));
    assert_eq!(i.crop(usize::MAX, 0, 2, 1), Err(ImageError::OutOfBounds));
    assert_eq!(i.crop(1, usize::MAX, 1, 2), Err(ImageError::OutOfBounds));
    assert_eq!(i.crop(0, 0, usize::MAX, 1), Err(ImageError::TooLarge));
}
