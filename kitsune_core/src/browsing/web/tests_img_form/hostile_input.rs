use super::*;

#[test]
fn hostile_img_and_form_markup_does_not_panic() {
    let junk = [
        "<img>",
        "<img src>",
        "<img src= width=-5 height=99999999999999999999>",
        "<img src='data:image/png;base64,' alt='\u{0}\u{1}\u{ffff}'>",
        "<form><form><input><button></form></form>",
        "<form action=<input name=x>>",
        "<input name=q size=-1 value='\u{ffff}'>",
        "<form><input type=hidden><input type=submit value=''><button></button></form>",
        "<img src=a width=1e9 height=0x10>",
        "<a href=><img src=x></a>",
    ];
    for j in junk {
        for zoom in [50, 100, 300] {
            let p = lay(j, 300, zoom, &Fixed(ImgState::Ready { w: 1, h: 1 }));
            assert!(p.height >= 0);
        }
        let p = lay(j, 300, 100, &NoImages);
        let _ = p.find("x", &FixedAdvance);
        let _ = p.select((0, 0), (100, 100), &FixedAdvance);
    }
}
