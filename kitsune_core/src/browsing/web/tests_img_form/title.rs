use super::*;

#[test]
fn title_is_extracted_and_collapsed() {
    let d = Doc::parse(
        "<html><head><title> Ol\u{e1}  mundo </title></head><body>x</body></html>".as_bytes(),
    );
    assert_eq!(d.title(), "Ol\u{e1} mundo");
}

#[test]
fn missing_or_empty_title() {
    assert_eq!(Doc::parse(b"<p>x</p>").title(), "");
    assert_eq!(Doc::parse(b"<title>  </title>").title(), "");
}

#[test]
fn title_is_not_painted() {
    let p = render(b"<title>Segredo</title><p>visivel</p>", 600);
    assert!(!texts(&p).iter().any(|t| t == "Segredo"));
}
