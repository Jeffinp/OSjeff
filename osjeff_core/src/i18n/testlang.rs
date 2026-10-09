//! Test helper: run a test in a chosen language without touching the global one.
//!
//! The language in effect is one global atomic and the host tests run in parallel, so a test
//! that flips it would race with every other test that reads text. Under `cfg(test)`
//! [`super::lang`] first looks at a per-thread override (one test = one thread), which a
//! [`LangGuard`] sets and restores. Tests of the global switch itself keep using
//! `tests::LANG_LOCK` and `set_lang`.

use super::Lang;
use core::cell::Cell;

std::thread_local! {
    static OVERRIDE: Cell<Option<Lang>> = const { Cell::new(None) };
}

/// The language this thread's test asked for, if any.
pub(crate) fn current() -> Option<Lang> {
    OVERRIDE.with(Cell::get)
}

/// While alive, [`super::lang`] answers `l` on this thread; dropping it restores what was there.
pub(crate) struct LangGuard(Option<Lang>);

impl LangGuard {
    pub(crate) fn new(l: Lang) -> Self {
        Self(OVERRIDE.with(|c| c.replace(Some(l))))
    }

    /// Change the language of this guard (what it restores on drop stays as it was).
    pub(crate) fn set(&mut self, l: Lang) {
        OVERRIDE.with(|c| c.set(Some(l)));
    }
}

impl Drop for LangGuard {
    fn drop(&mut self) {
        OVERRIDE.with(|c| c.set(self.0));
    }
}

#[test]
fn the_override_is_per_thread_and_restores() {
    let before = super::lang();
    {
        let _g = LangGuard::new(Lang::En);
        assert_eq!(super::lang(), Lang::En);
        assert_eq!(super::tr("meta.code"), "en");
        {
            let _g2 = LangGuard::new(Lang::Pt);
            assert_eq!(super::lang(), Lang::Pt);
        }
        assert_eq!(super::lang(), Lang::En);
        std::thread::spawn(|| assert_eq!(current(), None))
            .join()
            .unwrap();
    }
    assert_eq!(current(), None);
    assert_eq!(super::lang(), before);
}
