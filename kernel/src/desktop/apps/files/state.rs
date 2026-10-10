//! State of a file manager window: the view, the inline name field, drags, jobs and the preview.

use crate::desktop::*;
use alloc::boxed::Box;

/// What the inline name field of a file manager is for.
pub(crate) enum EditPurpose {
    /// Renaming the item at this path (a new file or folder is created first, then renamed).
    Rename(Vec<u8>),
}

/// The inline name editor (rename, and the first name of a new file or folder).
pub(crate) struct NameEdit {
    pub input: kitsune_core::fileman::TextInput,
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
    pub input: kitsune_core::fileman::TextInput,
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
    Place(kitsune_core::fileman::Place),
    Crumb(usize),
}

/// Items being dragged.
pub(crate) struct DragState {
    pub sources: Vec<Vec<u8>>,
    /// What the ghost shows: the first name and how many items.
    pub label: String,
    pub count: usize,
    pub kind: kitsune_core::appart::FileKind,
    pub over: DropHover,
    /// The operation a drop here would do (`None`: not a valid target).
    pub op: Option<kitsune_core::fileman::ui::DropOp>,
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
    pub kind: kitsune_core::appart::FileKind,
    pub kind_label: String,
    /// Label and value rows under the title.
    pub info: Vec<(String, String)>,
    /// An image, already scaled to the pane.
    pub image: Option<kitsune_core::raster::Surface>,
    /// The first lines of a text file.
    pub lines: Vec<String>,
    /// Why there is no picture or text (shown in secondary colour).
    pub note: Option<String>,
}

/// A file-manager window.
pub(crate) struct FilesState {
    pub view: kitsune_core::fileman::FileView,
    pub mode: kitsune_core::fileman::ui::ViewMode,
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
    pub scroller: kitsune_core::fileman::ui::Scroller,
    pub scroll_fade: kitsune_core::widgets::ScrollbarFade,
    pub hover: Option<kitsune_core::fileman::ui::Hit>,
    pub hover_t: kitsune_core::anim::Tween,
    pub gesture: Gesture,
    /// Sheet transition (0 closed .. 1 open), content fade-in after a folder change.
    pub sheet_t: kitsune_core::anim::Tween,
    pub enter_t: kitsune_core::anim::Tween,
    /// `view.nav_gen` the scroll position belongs to.
    pub seen_nav: u32,
}

impl FilesState {
    pub(crate) fn new() -> Self {
        FilesState {
            // A new window opens in the signed-in user's home when it exists, else the root.
            view: {
                let home = crate::desktop::accounts_home();
                if crate::desktop::services::vfs::exists(&home) {
                    kitsune_core::fileman::FileView::at(&home)
                } else {
                    kitsune_core::fileman::FileView::new()
                }
            },
            mode: kitsune_core::fileman::ui::ViewMode::List,
            preview_open: false,
            preview: None,
            search: SearchField {
                input: kitsune_core::fileman::TextInput::new(b"", 64),
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
            scroller: kitsune_core::fileman::ui::Scroller::new(),
            scroll_fade: kitsune_core::widgets::ScrollbarFade::new(),
            hover: None,
            hover_t: kitsune_core::anim::Tween::at(1.0),
            gesture: Gesture::None,
            sheet_t: kitsune_core::anim::Tween::at(0.0),
            enter_t: kitsune_core::anim::Tween::at(1.0),
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
        self.sheet_t = kitsune_core::anim::Tween::at(0.0);
        self.sheet_t
            .retarget(1.0, 0.24, kitsune_core::anim::curves::ENTER);
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
