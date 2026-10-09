use super::*;

#[test]
fn start_page_cannot_be_bookmarked() {
    let mut b = Browser::new();
    assert_eq!(b.toggle_bookmark(), None);
    assert!(!b.is_bookmarked());
}

#[test]
fn ctrl_d_toggles_the_current_page() {
    let mut b = browser_with_history(&["http://a.test/x"]);
    assert!(!b.is_bookmarked());
    assert_eq!(b.toggle_bookmark(), Some(true));
    assert!(b.is_bookmarked());
    assert_eq!(b.bookmarks().len(), 1);
    assert_eq!(b.toggle_bookmark(), Some(false));
    assert!(!b.is_bookmarked());
    assert!(b.bookmarks().is_empty());
}

#[test]
fn bookmark_uses_the_page_title_or_the_url() {
    let mut b = browser_with_history(&["http://a.test/x"]);
    b.set_page_title("Página A");
    b.toggle_bookmark();
    assert_eq!(b.bookmarks()[0].title, "Página A");
    b.toggle_bookmark();
    b.set_page_title("");
    b.toggle_bookmark();
    assert_eq!(b.bookmarks()[0].title, "http://a.test/x");
}

#[test]
fn memory_store_rules() {
    let mut s = MemoryBookmarks::default();
    let bm = |u: &str| Bookmark {
        url: u.into(),
        title: String::new(),
    };
    assert!(s.add(bm("http://a/")));
    assert!(!s.add(bm("http://a/")), "no duplicates");
    assert!(s.contains("http://a/"));
    assert!(s.remove("http://a/"));
    assert!(!s.remove("http://a/"));
    for i in 0..MAX_BOOKMARKS {
        assert!(s.add(bm(&alloc::format!("http://h/{i}"))));
    }
    assert!(!s.add(bm("http://one-too-many/")));
    assert_eq!(s.all().len(), MAX_BOOKMARKS);
}

#[test]
fn a_custom_store_is_used() {
    use alloc::rc::Rc;
    use core::cell::RefCell;
    struct Shared(Rc<RefCell<Vec<Bookmark>>>);
    impl BookmarkStore for Shared {
        fn all(&self) -> Vec<Bookmark> {
            self.0.borrow().clone()
        }
        fn add(&mut self, b: Bookmark) -> bool {
            self.0.borrow_mut().push(b);
            true
        }
        fn remove(&mut self, url: &str) -> bool {
            self.0.borrow_mut().retain(|b| b.url != url);
            true
        }
    }
    let backing = Rc::new(RefCell::new(Vec::new()));
    let mut b = Browser::with_store(Box::new(Shared(backing.clone())));
    b.open(b"http://z.test/");
    b.take_request();
    b.loaded_with(Conn::Plain, false);
    b.toggle_bookmark();
    assert_eq!(backing.borrow().len(), 1);
    assert!(b.is_bookmarked());
}
