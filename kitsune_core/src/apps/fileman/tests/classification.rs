use super::*;

#[test]
fn files_are_classified_by_extension() {
    assert_eq!(classify(b"a.PNG"), FileClass::Image);
    assert_eq!(classify(b"a.bmp"), FileClass::Image);
    assert_eq!(classify(b"x.ppm"), FileClass::Image);
    assert_eq!(classify(b"app.wasm"), FileClass::Wasm);
    assert_eq!(classify(b"notes.txt"), FileClass::Text);
    assert_eq!(classify(b"Makefile"), FileClass::Text);
    assert_eq!(classify(b"a.xyz"), FileClass::Other);
    assert!(is_image(b"foto.png"));
    assert!(!is_image(b"foto.png.txt"));
}

#[test]
fn text_sniffing() {
    assert!(looks_like_text(b"hello\nworld\t!"));
    assert!(looks_like_text("relatório".as_bytes()));
    assert!(!looks_like_text(b"abc\0def"));
    assert!(!looks_like_text(&[1u8; 100]));
    assert!(looks_like_text(b""));
}
