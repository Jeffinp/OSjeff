//! App instances: what lives inside each window.
//!
//! The desktop used to keep one field per app (`term`, `editor`, ...) and a
//! fixed window slot per [`Kind`]. Now every window owns an [`Inst`] holding its
//! own [`App`] state, so any number of terminals, editors, file managers and
//! calculators can be open at once, each with its own process-table entry.
//!
//! # Adding an app
//!
//! 1. Add a [`Kind`] variant and fill in its `const fn` metadata below (title,
//!    process name, default size, minimum size, `multi`, `resizable`, icon).
//! 2. Add an [`App`] variant holding the per-window state (box big states so
//!    moving a window record stays cheap) and construct it in [`App::new`].
//! 3. Draw it in `Desktop::draw_window` (`render.rs`) and handle keys / clicks in
//!    `input.rs`. Everything else — z-order, focus, minimize / maximize / resize,
//!    Alt+Tab, the dock indicator, the process entry (`name`, `name 2`, ...) and
//!    teardown on close — is generic and needs no change.
//! 4. Add it to `taskbar::DEFAULT_PINNED` if it should start pinned to the taskbar; every app
//!    appears in the Apps overlay and in Busca on its own, and while it runs the taskbar
//!    shows it.
//!
//! Instance state is plain data; nothing here allocates per frame. The
//! per-instance heap objects (`Box`, `Vec`, `String`) are created when the window
//! opens and freed when it is destroyed.

use super::*;
use alloc::boxed::Box;
use osjeff_core::i18n::{Lang, tr, tr_in};
use osjeff_core::tk;

/// Which app a window runs. The order is the Apps overlay / menu order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Kind {
    Terminal,
    Editor,
    TaskMgr,
    Calculator,
    Browser,
    WasmApp,
    Files,
    Settings,
    LogViewer,
    Viewer,
    /// The component gallery (Ctrl+Alt+G): not listed anywhere else.
    Gallery,
}

impl Kind {
    pub(crate) const ALL: [Kind; 10] = [
        Kind::Terminal,
        Kind::Editor,
        Kind::TaskMgr,
        Kind::Calculator,
        Kind::Browser,
        Kind::WasmApp,
        Kind::Files,
        Kind::Settings,
        Kind::LogViewer,
        Kind::Viewer,
    ];

    /// Does the window's content change on its own every second (so the
    /// per-second tick must repaint it)?
    pub(crate) const fn is_live(self) -> bool {
        matches!(self, Kind::TaskMgr | Kind::Settings | Kind::LogViewer)
    }

    /// Process-table name of the first instance (`shell`; later ones get `shell 2`...).
    pub(crate) const fn proc_name(self) -> &'static str {
        match self {
            Kind::Terminal => "shell",
            Kind::Editor => "editor",
            Kind::TaskMgr => "taskmgr",
            Kind::Calculator => "calc",
            Kind::Browser => "browser",
            Kind::WasmApp => "wasmapp",
            Kind::Files => "files",
            Kind::Settings => "settings",
            Kind::LogViewer => "syslog",
            Kind::Viewer => "viewer",
            Kind::Gallery => "gallery",
        }
    }

    /// Catalog key of the app's name.
    pub(crate) const fn name_key(self) -> &'static str {
        match self {
            Kind::Terminal => tk!("app.terminal"),
            Kind::Editor => tk!("app.editor"),
            Kind::TaskMgr => tk!("app.tasks"),
            Kind::Calculator => tk!("app.calculator"),
            Kind::Browser => tk!("app.browser"),
            Kind::WasmApp => tk!("app.wasm"),
            Kind::Files => tk!("app.files"),
            Kind::Settings => tk!("app.settings"),
            Kind::LogViewer => tk!("app.log"),
            Kind::Viewer => tk!("app.viewer"),
            Kind::Gallery => tk!("app.gallery"),
        }
    }

    /// Title-bar text of the first instance, in language `l`.
    pub(crate) fn title_in(self, l: Lang) -> &'static str {
        match self {
            Kind::WasmApp => tr_in(l, tk!("app.wasm_title")),
            _ => tr_in(l, self.name_key()),
        }
    }

    /// Name in menus, the app bar's tooltips, the Apps overlay and Busca.
    pub(crate) fn label(self) -> &'static str {
        tr(self.name_key())
    }

    /// Name in language `l` (Busca also matches the English name in Portuguese).
    pub(crate) fn label_in(self, l: Lang) -> &'static str {
        tr_in(l, self.name_key())
    }

    pub(crate) const fn icon(self) -> Icon {
        match self {
            Kind::Terminal => Icon::Terminal,
            Kind::Editor => Icon::Editor,
            Kind::TaskMgr => Icon::TaskMgr,
            Kind::Calculator => Icon::Calculator,
            Kind::Browser => Icon::Browser,
            Kind::WasmApp => Icon::WasmApp,
            Kind::Files => Icon::Files,
            Kind::Settings => Icon::Settings,
            Kind::LogViewer => Icon::Log,
            Kind::Viewer => Icon::Viewer,
            Kind::Gallery => Icon::WasmApp,
        }
    }

    /// May several windows of this kind be open at once? The task manager and the
    /// browser (one NIC, one fetcher thread) are single-instance for now: launching
    /// them again focuses the existing window. WASM apps are multi-instance: each
    /// window is its own `AppManager` instance.
    pub(crate) const fn multi(self) -> bool {
        matches!(
            self,
            Kind::Terminal
                | Kind::Editor
                | Kind::Calculator
                | Kind::Files
                | Kind::WasmApp
                | Kind::Viewer
        )
    }

    /// Position and size of the first instance (later ones cascade from it).
    pub(crate) const fn default_rect(self) -> Rect {
        match self {
            Kind::Terminal => Rect::new(70, 80, 600, 360),
            Kind::Editor => Rect::new(610, 110, 560, 350),
            Kind::TaskMgr => Rect::new(190, 52, 860, 592),
            Kind::Calculator => Rect::new(470, 100, 320, 520),
            Kind::Browser => Rect::new(150, 60, 916, 560),
            Kind::WasmApp => Rect::new(240, 130, 720, 470),
            Kind::Files => Rect::new(220, 110, 860, 520),
            Kind::Viewer => Rect::new(200, 90, 820, 540),
            Kind::Gallery => Rect::new(160, 70, 900, 600),
            Kind::Settings => Rect::new(220, 56, 820, 596),
            Kind::LogViewer => Rect::new(200, 80, 880, 540),
        }
    }

    /// Smallest size the window can be resized to.
    pub(crate) const fn min_size(self) -> (i32, i32) {
        match self {
            Kind::Terminal => (320, 180),
            Kind::Editor => (320, 200),
            Kind::TaskMgr => (700, 460),
            Kind::Calculator => (280, 440),
            Kind::Browser => (420, 260),
            Kind::WasmApp => (720, 470),
            Kind::Files => (640, 340),
            Kind::Viewer => (520, 340),
            Kind::Gallery => (560, 380),
            Kind::Settings => (720, 480),
            Kind::LogViewer => (720, 360),
        }
    }

    /// Whether the window can be resized / maximized (WASM windows decide per app,
    /// from the manifest: see `Desktop::open_wasm_app`).
    pub(crate) const fn resizable(self) -> bool {
        true
    }
}

/// What the pointer is over in a browser window (hover looks follow it).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum BrowserHover {
    #[default]
    None,
    Back,
    Forward,
    Reload,
    NewTab,
    Star,
    Shield,
    Bar,
    /// A tab of the strip (index into the tabs, not the strip).
    Tab(usize),
    TabClose(usize),
    Suggestion(usize),
    /// A row of the page's context menu.
    MenuRow(usize),
    /// A tile or row of the start page.
    Tile(usize),
    Recent(usize),
    /// A button of the find bar: previous, next, close.
    FindPrev,
    FindNext,
    FindClose,
    /// The error page's buttons.
    Retry,
    Proceed,
}

/// One tab of a browser window: its own history, page, scroll, forms and find state. Only
/// the active tab is laid out and painted; the others keep their parsed document.
pub(crate) struct TabData {
    /// Never reused within a window: a result from the network finds its tab by this.
    pub id: u32,
    pub browser: osjeff_core::Browser,
    /// The parsed document (kept so the page can be laid out again without parsing).
    pub doc: Option<osjeff_core::web::Doc>,
    pub page: Option<osjeff_core::web::Page>,
    /// The scroll offset being painted (the spring below eases it to its target).
    pub scroll: i32,
    pub scroll_spring: osjeff_core::anim::Spring,
    pub scroll_bar: osjeff_core::widgets::ScrollbarFade,
    /// Viewport width `page` was laid out for.
    pub layout_w: i32,
    /// Cache key of each `page.images` entry (`None`: not fetchable).
    pub img_keys: Vec<Option<String>>,
    /// Page zoom in percent.
    pub zoom: u16,
    /// What the user typed into the page's form controls.
    pub forms: osjeff_core::web::form::FormState,
    /// Ctrl+F bar and matches.
    pub find: osjeff_core::web::find::FindBar,
    /// Start of a mouse selection (page coordinates) and the selected text.
    pub sel_anchor: Option<(i32, i32)>,
    pub sel: Option<osjeff_core::web::textops::Selection>,
    /// The link under the pointer (index into the page's links), for the hover underline.
    pub hover_link: Option<usize>,
    /// The certificate summary of the page on screen (https only).
    pub cert: Option<osjeff_core::browser::CertInfo>,
    /// The progress bar under the omnibox.
    pub load: osjeff_core::browser::motion::LoadBar,
    /// Fill of the favourite star, 0 (outline) to 1 (filled).
    pub star: osjeff_core::anim::Tween,
    /// The load in flight was stopped: its result is dropped when it arrives.
    pub cancelled: bool,
    /// `perf-trace`: when the page was handed over, until its first paint is reported.
    pub trace_t0: core::cell::Cell<u64>,
}

/// A tab as the strip shows it: it grows when it opens and shrinks when it closes (a closed
/// tab stays as a "ghost" until its animation ends).
pub(crate) struct StripEntry {
    /// The tab's id, or `None` for a ghost.
    pub id: Option<u32>,
    pub weight: osjeff_core::anim::Tween,
    /// What a ghost still shows.
    pub title: String,
    pub badge: char,
}

/// Browser window state: the tabs (each with its own page), what they share (the image
/// cache, the fetcher's single slot) and the chrome's own transient state. `Deref`s to the
/// active tab so the code that works on "the page on screen" just says `b.page`.
pub(crate) struct BrowserState {
    pub tabs: osjeff_core::browser::tabs::TabList<TabData>,
    /// Pictures of the page being shown (and a few recent ones), shared by the tabs.
    pub images: osjeff_core::web::imgcache::ImageCache,
    /// The picture the fetcher is working on, and the tab that asked for it.
    pub img_inflight: Option<(u32, String)>,
    /// The tab whose page request the fetcher has.
    pub req_tab: Option<u32>,
    pub next_id: u32,
    /// The strip in visual order (ghosts included).
    pub strip: Vec<StripEntry>,
    /// Height of the tab strip while it appears and disappears.
    pub strip_h: osjeff_core::anim::Tween,
    pub hover: BrowserHover,
    /// The security popover is open.
    pub popover: bool,
    /// The page's context menu: where it opened, what it can do.
    pub ctx: Option<PageMenu>,
    /// The line over the bottom of the page ("Favorito adicionado") and its fade.
    pub notice: Option<String>,
    pub notice_flash: osjeff_core::browser::motion::Flash,
    /// The zoom pill's fade.
    pub zoom_flash: osjeff_core::browser::motion::Flash,
    /// The frame where the omnibox suggestions, the popover, the find bar and the menu take
    /// their blurred backdrop from.
    pub glass: [super::glass::BackdropSlot; 4],
    /// The page area as last painted (see `browser_paint`).
    pub cache: core::cell::RefCell<super::browser_paint::PaintCache>,
    /// Bumped by everything that changes how the page area looks, so the cache knows.
    pub rev: u64,
}

/// The page's context menu.
pub(crate) struct PageMenu {
    pub x: i32,
    pub y: i32,
    /// The link under the pointer when it opened.
    pub link: Option<String>,
    /// Page coordinates of the click (to ask what was under it).
    pub items: Vec<(PageCmd, &'static str, bool)>,
}

/// What a context menu row does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum PageCmd {
    Copy,
    OpenLink,
    CopyLink,
    Back,
    Reload,
    Bookmark,
}

impl core::ops::Deref for BrowserState {
    type Target = TabData;
    fn deref(&self) -> &TabData {
        self.tabs.active()
    }
}

impl core::ops::DerefMut for BrowserState {
    fn deref_mut(&mut self) -> &mut TabData {
        self.tabs.active_mut()
    }
}

impl TabData {
    pub(crate) fn new(id: u32, browser: osjeff_core::Browser) -> TabData {
        TabData {
            id,
            browser,
            doc: None,
            page: None,
            scroll: 0,
            scroll_spring: osjeff_core::anim::Spring::pixels(0.0, 260.0, 30.0),
            scroll_bar: osjeff_core::widgets::ScrollbarFade::new(),
            layout_w: 0,
            img_keys: Vec::new(),
            zoom: 100,
            forms: osjeff_core::web::form::FormState::default(),
            find: osjeff_core::web::find::FindBar::new(),
            sel_anchor: None,
            sel: None,
            hover_link: None,
            cert: None,
            load: osjeff_core::browser::motion::LoadBar::new(),
            star: osjeff_core::anim::Tween::at(0.0),
            cancelled: false,
            trace_t0: core::cell::Cell::new(0),
        }
    }
}

impl BrowserState {
    pub(crate) fn new() -> BrowserState {
        let first = TabData::new(1, osjeff_core::Browser::with_store(new_bookmark_store()));
        BrowserState {
            tabs: osjeff_core::browser::tabs::TabList::new(first),
            images: osjeff_core::web::imgcache::ImageCache::new(),
            img_inflight: None,
            req_tab: None,
            next_id: 2,
            strip: alloc::vec![StripEntry {
                id: Some(1),
                weight: osjeff_core::anim::Tween::at(1.0),
                title: String::new(),
                badge: ' ',
            }],
            strip_h: osjeff_core::anim::Tween::at(0.0),
            hover: BrowserHover::None,
            popover: false,
            ctx: None,
            notice: None,
            notice_flash: osjeff_core::browser::motion::Flash::new(),
            zoom_flash: osjeff_core::browser::motion::Flash::new(),
            glass: Default::default(),
            cache: core::cell::RefCell::new(Default::default()),
            rev: 1,
        }
    }
}

/// The single place that decides where the browser's favourites live: the file
/// `/home/.bookmarks` on the desktop volume (written after every change; a missing or
/// damaged file starts an empty list).
pub(crate) fn new_bookmark_store() -> Box<dyn osjeff_core::browser::BookmarkStore> {
    const PATH: &[u8] = b"/home/.bookmarks";
    let text = super::vfs::read_file(PATH).unwrap_or_default();
    Box::new(osjeff_core::browser::SavedBookmarks::load(&text, |t| {
        if !super::vfs::exists(b"/home") {
            let _ = super::vfs::mkdir(b"/home");
        }
        if super::vfs::write_file(PATH, t).is_err() {
            crate::klog!(Warn, "bookmarks: could not be saved");
        }
    }))
}

/// What the inline name field of a file manager is for.
pub(crate) enum EditPurpose {
    /// Renaming the item at this path (a new file or folder is created first, then renamed).
    Rename(Vec<u8>),
}

/// The inline name editor (rename, and the first name of a new file or folder).
pub(crate) struct NameEdit {
    pub input: osjeff_core::fileman::TextInput,
    pub purpose: EditPurpose,
    /// Tick of the last key, for the caret.
    pub last_input: u64,
}

/// A question the file manager waits on (Enter confirms, Esc cancels).
pub(crate) enum Confirm {
    /// Delete these paths for good.
    Purge(Vec<Vec<u8>>),
    /// Delete these trash items for good (by trash id).
    PurgeTrash(Vec<Vec<u8>>),
    EmptyTrash,
}

/// A copy running in steps (see `Desktop::step_file_jobs`).
pub(crate) struct Job {
    pub copy: vfs::CopyJob,
    /// Catalog key of the sheet's title (looked up when drawn, so it follows the language).
    pub label: &'static str,
    /// Tick the job started: the progress sheet appears only for copies that take a moment.
    pub started: u64,
}

/// The search field of a file manager.
pub(crate) struct SearchField {
    pub input: osjeff_core::fileman::TextInput,
    /// The field has the keyboard.
    pub focused: bool,
    pub last_input: u64,
}

/// Where a dragged group of items would land.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum DropHover {
    None,
    /// A folder row or icon (index into the rows).
    Item(usize),
    Place(osjeff_core::fileman::Place),
    Crumb(usize),
}

/// Items being dragged.
pub(crate) struct DragState {
    pub sources: Vec<Vec<u8>>,
    /// What the ghost shows: the first name and how many items.
    pub label: String,
    pub count: usize,
    pub kind: osjeff_core::appart::FileKind,
    pub over: DropHover,
    /// The operation a drop here would do (`None`: not a valid target).
    pub op: Option<osjeff_core::fileman::ui::DropOp>,
    pub pos: (i32, i32),
}

/// What the pointer is doing with the left button down in a file manager.
pub(crate) enum Gesture {
    None,
    /// Pressed on an item: a drag once the pointer travels, else a click when released.
    Press {
        item: usize,
        at: (i32, i32),
        /// The item was already selected: the selection collapses to it on release.
        collapse: bool,
    },
    Drag(Box<DragState>),
    /// Rubber band: anchor in content coordinates, the selection it started from.
    Band {
        anchor: (i32, i32),
        cur: (i32, i32),
        base: Vec<usize>,
        additive: bool,
    },
    /// Dragging the scrollbar thumb; the pointer's offset inside the thumb.
    Thumb {
        grab: i32,
    },
}

/// What the preview pane shows for the selected item.
pub(crate) struct PreviewData {
    pub path: Vec<u8>,
    pub name: String,
    pub kind: osjeff_core::appart::FileKind,
    pub kind_label: String,
    /// Label and value rows under the title.
    pub info: Vec<(String, String)>,
    /// An image, already scaled to the pane.
    pub image: Option<osjeff_core::raster::Surface>,
    /// The first lines of a text file.
    pub lines: Vec<String>,
    /// Why there is no picture or text (shown in secondary colour).
    pub note: Option<String>,
}

/// A file-manager window.
pub(crate) struct FilesState {
    pub view: osjeff_core::fileman::FileView,
    pub mode: osjeff_core::fileman::ui::ViewMode,
    pub preview_open: bool,
    pub preview: Option<Box<PreviewData>>,
    pub search: SearchField,
    pub input: Option<NameEdit>,
    pub confirm: Option<Confirm>,
    /// Lines of the information sheet while it is open.
    pub props: Option<Vec<String>>,
    pub job: Option<Job>,
    /// Last status message and whether it is an error, with the tick it appeared.
    pub msg: Option<(String, bool)>,
    pub msg_tick: u64,
    pub usage: vfs::Usage,
    pub scroller: osjeff_core::fileman::ui::Scroller,
    pub scroll_fade: osjeff_core::widgets::ScrollbarFade,
    pub hover: Option<osjeff_core::fileman::ui::Hit>,
    pub hover_t: osjeff_core::anim::Tween,
    pub gesture: Gesture,
    /// Sheet transition (0 closed .. 1 open), content fade-in after a folder change.
    pub sheet_t: osjeff_core::anim::Tween,
    pub enter_t: osjeff_core::anim::Tween,
    /// `view.nav_gen` the scroll position belongs to.
    pub seen_nav: u32,
}

impl FilesState {
    pub(crate) fn new() -> Self {
        FilesState {
            view: osjeff_core::fileman::FileView::new(),
            mode: osjeff_core::fileman::ui::ViewMode::List,
            preview_open: false,
            preview: None,
            search: SearchField {
                input: osjeff_core::fileman::TextInput::new(b"", 64),
                focused: false,
                last_input: 0,
            },
            input: None,
            confirm: None,
            props: None,
            job: None,
            msg: None,
            msg_tick: 0,
            usage: vfs::Usage::default(),
            scroller: osjeff_core::fileman::ui::Scroller::new(),
            scroll_fade: osjeff_core::widgets::ScrollbarFade::new(),
            hover: None,
            hover_t: osjeff_core::anim::Tween::at(1.0),
            gesture: Gesture::None,
            sheet_t: osjeff_core::anim::Tween::at(0.0),
            enter_t: osjeff_core::anim::Tween::at(1.0),
            seen_nav: 0,
        }
    }

    pub(crate) fn say(&mut self, text: &str, error: bool) {
        self.msg = Some((String::from(text), error));
        self.msg_tick = crate::interrupts::ticks();
    }

    /// The search field is shown expanded: it has the keyboard or holds a query.
    pub(crate) fn search_is_open(&self) -> bool {
        self.search.focused || !self.search.input.text().is_empty()
    }

    /// A sheet is up (question, information, or a copy long enough to show progress).
    pub(crate) fn sheet_open(&self) -> bool {
        self.confirm.is_some() || self.props.is_some() || self.copy_sheet()
    }

    /// The progress sheet of a copy that has been running for a moment.
    pub(crate) fn copy_sheet(&self) -> bool {
        self.job
            .as_ref()
            .is_some_and(|j| crate::interrupts::ticks().saturating_sub(j.started) > 60)
    }

    /// Start the sheet's slide-in.
    pub(crate) fn open_sheet(&mut self) {
        self.sheet_t = osjeff_core::anim::Tween::at(0.0);
        self.sheet_t
            .retarget(1.0, 0.24, osjeff_core::anim::curves::ENTER);
    }

    /// Whether something in the window moves on its own and needs frames.
    pub(crate) fn animating(&self) -> bool {
        let now = crate::interrupts::ticks();
        !self.scroller.at_rest()
            || !self.hover_t.finished()
            || !self.sheet_t.finished()
            || !self.enter_t.finished()
            || self.job.is_some()
            || self.scroll_fade.active((now.wrapping_mul(4)) as u32)
            || (!matches!(self.gesture, Gesture::None))
            || self
                .input
                .as_ref()
                .is_some_and(|e| appui::caret_animating(e.last_input))
            || (self.search.focused && appui::caret_animating(self.search.last_input))
    }
}

/// A filmstrip thumbnail: not made yet, made, or impossible (too big or unreadable).
pub(crate) enum Thumb {
    Pending,
    Ready(osjeff_core::raster::Surface),
    Missing,
}

/// An image-viewer window.
pub(crate) struct ViewerState {
    pub path: Vec<u8>,
    pub image: Option<osjeff_core::image::Image>,
    /// Box-filtered copy for zooms below 100 %: `(zoom, image)`.
    pub scaled: Option<(u32, osjeff_core::image::Image)>,
    pub opaque: bool,
    /// Where the zoom and pan are heading (the drawn values follow with springs).
    pub view: osjeff_core::viewer::View,
    pub list: osjeff_core::viewer::ImageList,
    pub format: Option<osjeff_core::image::Format>,
    pub file_bytes: u64,
    /// Why the file could not be shown.
    pub error: Option<[String; 2]>,
    pub show_info: bool,
    /// The "save as" sheet.
    pub save: Option<osjeff_core::fileman::TextInput>,
    pub msg: Option<(String, bool)>,
    /// The zoom (permille) and pan being drawn.
    pub zoom_s: osjeff_core::anim::Spring,
    pub pan_x_s: osjeff_core::anim::Spring,
    pub pan_y_s: osjeff_core::anim::Spring,
    /// Extra turn (degrees, clockwise) of the drawn picture while a rotation animates.
    pub rot: osjeff_core::anim::Tween,
    /// The picture fading in after a change of image.
    pub enter_t: osjeff_core::anim::Tween,
    pub info_t: osjeff_core::anim::Tween,
    pub sheet_t: osjeff_core::anim::Tween,
    pub hover: Option<osjeff_core::viewer::ui::Hit>,
    pub hover_t: osjeff_core::anim::Tween,
    pub inertia: osjeff_core::viewer::ui::Inertia,
    /// Tick of the last drag event (for the speed of a flick).
    pub drag_tick: u64,
    pub slideshow: osjeff_core::viewer::ui::Slideshow,
    pub thumbs: Vec<Thumb>,
    pub strip_scroll: osjeff_core::anim::Spring,
    /// Tick the last thumbnail was made.
    pub thumb_tick: u64,
    /// The viewport the zoom was last fitted for: a change jumps instead of animating.
    pub vp_seen: (i32, i32),
}

impl ViewerState {
    pub(crate) fn new() -> Self {
        use osjeff_core::anim::{Spring, Tween};
        ViewerState {
            path: Vec::new(),
            image: None,
            scaled: None,
            opaque: true,
            view: osjeff_core::viewer::View::default(),
            list: osjeff_core::viewer::ImageList::default(),
            format: None,
            file_bytes: 0,
            error: None,
            show_info: false,
            save: None,
            msg: None,
            zoom_s: Spring::pixels(1000.0, 240.0, 31.0),
            pan_x_s: Spring::pixels(0.0, 240.0, 31.0),
            pan_y_s: Spring::pixels(0.0, 240.0, 31.0),
            rot: Tween::at(0.0),
            enter_t: Tween::at(1.0),
            info_t: Tween::at(0.0),
            sheet_t: Tween::at(0.0),
            hover: None,
            hover_t: Tween::at(1.0),
            inertia: osjeff_core::viewer::ui::Inertia::new(),
            drag_tick: 0,
            slideshow: osjeff_core::viewer::ui::Slideshow::new(),
            thumbs: Vec::new(),
            strip_scroll: Spring::pixels(0.0, 260.0, 32.0),
            thumb_tick: 0,
            vp_seen: (0, 0),
        }
    }

    /// Whether something in the window moves on its own and needs frames.
    pub(crate) fn animating(&self) -> bool {
        !self.zoom_s.at_rest()
            || !self.pan_x_s.at_rest()
            || !self.pan_y_s.at_rest()
            || !self.rot.finished()
            || !self.enter_t.finished()
            || !self.info_t.finished()
            || !self.sheet_t.finished()
            || !self.hover_t.finished()
            || !self.strip_scroll.at_rest()
            || self.inertia.active()
            || self.thumbs_pending()
    }

    /// Thumbnails still to make (the window keeps running frames until they are done).
    pub(crate) fn thumbs_pending(&self) -> bool {
        self.list.len() > 1 && self.thumbs.iter().any(|t| matches!(t, Thumb::Pending))
    }
}

/// A WASM app window: the `AppManager` instance behind it and which package it is.
pub(crate) struct WasmWin {
    /// Handle in the manager (`0` = none).
    pub id: crate::wasm::AppId,
    /// Manifest id of the package (`snake`, `notes`, ...).
    pub app_id: String,
}

/// Title-bar text of instance `index` of `kind` (`OSJEFF SHELL`, `OSJEFF SHELL 2`...).
pub(crate) fn base_title(kind: Kind, index: u8) -> String {
    base_title_in(osjeff_core::i18n::lang(), kind, index)
}

/// [`base_title`] in language `l`.
pub(crate) fn base_title_in(l: Lang, kind: Kind, index: u8) -> String {
    let mut title = String::from(kind.title_in(l));
    if index > 1 {
        // The same " N" suffix as the process name.
        let mut tmp = [0u8; 16];
        let k = numbered_name("", index, &mut tmp);
        title.push_str(core::str::from_utf8(&tmp[..k]).unwrap_or(""));
    }
    title
}

/// Per-window app state.
pub(crate) enum App {
    Terminal(Box<TermState>),
    Editor(Box<EditorState>),
    Tarefas(Box<TarefasState>),
    Calculator(Box<calc_ui::CalcState>),
    Browser(Box<BrowserState>),
    Wasm(Box<WasmWin>),
    Files(Box<FilesState>),
    Viewer(Box<ViewerState>),
    Settings(Box<SettingsState>),
    Log(Box<LogState>),
    Gallery(Box<gallery::GalleryState>),
}

impl App {
    /// Fresh state for a new window of `kind`.
    pub(crate) fn new(kind: Kind) -> App {
        match kind {
            Kind::Terminal => App::Terminal(Box::new(TermState::new())),
            Kind::Editor => App::Editor(Box::new(EditorState::new())),
            Kind::TaskMgr => App::Tarefas(Box::new(TarefasState::new(0))),
            Kind::Calculator => App::Calculator(Box::new(calc_ui::CalcState::new())),
            Kind::Browser => App::Browser(Box::new(BrowserState::new())),
            Kind::WasmApp => App::Wasm(Box::new(WasmWin {
                id: 0,
                app_id: String::new(),
            })),
            Kind::Files => App::Files(Box::new(FilesState::new())),
            Kind::Viewer => App::Viewer(Box::new(ViewerState::new())),
            Kind::Settings => App::Settings(Box::new(SettingsState::new())),
            Kind::LogViewer => App::Log(Box::new(LogState::new())),
            Kind::Gallery => App::Gallery(Box::new(gallery::GalleryState::new())),
        }
    }

    pub(crate) fn kind(&self) -> Kind {
        match self {
            App::Terminal(_) => Kind::Terminal,
            App::Editor(_) => Kind::Editor,
            App::Tarefas(_) => Kind::TaskMgr,
            App::Calculator(_) => Kind::Calculator,
            App::Browser(_) => Kind::Browser,
            App::Wasm(_) => Kind::WasmApp,
            App::Files(_) => Kind::Files,
            App::Settings(_) => Kind::Settings,
            App::Log(_) => Kind::LogViewer,
            App::Viewer(_) => Kind::Viewer,
            App::Gallery(_) => Kind::Gallery,
        }
    }
}

/// What a window record carries: the app, its process, and its identity.
pub(crate) struct Inst {
    pub app: App,
    /// Process-table id (`0` = none, if the table was full).
    pub pid: u16,
    /// 1-based instance number within its kind (`shell` = 1, `shell 2` = 2...).
    pub index: u8,
    pub title: String,
    /// TSC cycles spent drawing this window since the last once-a-second sample
    /// (Tarefas' per-app figure).
    pub cost: core::cell::Cell<u64>,
    /// Draw cost of the last full second, in tenths of a percent of wall time.
    pub cost_pm: core::cell::Cell<u16>,
}

impl Inst {
    pub(crate) fn kind(&self) -> Kind {
        self.app.kind()
    }
}

/// A window record of this desktop.
pub(crate) type Win = osjeff_core::winman::Window<Inst>;

pub(crate) use osjeff_core::winman::numbered_name;
