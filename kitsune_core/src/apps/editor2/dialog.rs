//! The questions the editor window asks outside the text itself: the Open /
//! Save-as file picker, the "unsaved changes" choice and the status line.
//!
//! Pure state machines: the kernel feeds [`KeyEvent`]s (and mouse rows), lists
//! folders through the VFS when asked to, and draws what the structs expose.
//! Nothing here touches a filesystem, so every rule is unit-tested.

use super::{Eol, Status};
use crate::apps::fileman::natural_cmp;
use crate::apps::shell::fs::normalize;
use crate::system::input::{KeyCode, KeyEvent};
use alloc::string::String;
use alloc::vec::Vec;

/// Longest path or name the picker's field accepts, in characters.
pub const MAX_FIELD: usize = 255;

/// What the picker is for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PickMode {
    Open,
    SaveAs,
}

/// One line of the folder listing.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PickRow {
    pub name: String,
    pub dir: bool,
    pub size: u64,
}

/// What the kernel must do after a picker key or click.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum PickEvent {
    /// Nothing changed.
    None,
    /// Redraw the dialog.
    Redraw,
    /// List this folder (absolute path) and call [`Picker::set_entries`] (or
    /// [`Picker::set_error`] when it cannot be listed).
    Navigate(String),
    /// The user chose this absolute path: open it, or save to it. For a save
    /// the kernel checks whether it exists and, if so, calls
    /// [`Picker::confirm_overwrite`].
    Choose(String),
    /// The user agreed to replace the file asked about.
    Overwrite(String),
    /// The dialog was cancelled (Esc).
    Cancel,
}

/// The Open / Save-as dialog's state.
#[derive(Clone, Debug)]
pub struct Picker {
    pub mode: PickMode,
    dir: String,
    rows: Vec<PickRow>,
    sel: usize,
    scroll: usize,
    visible: usize,
    field: Vec<char>,
    caret: usize,
    error: Option<String>,
    ask: Option<String>,
    /// The field still holds the name it started with: the first typed character replaces it
    /// (as a selected text would), any other key keeps it.
    fresh: bool,
}

impl Picker {
    /// A picker showing `dir` (not listed yet: call [`Picker::set_entries`]),
    /// the name field starting as `name`.
    pub fn new(mode: PickMode, dir: &str, name: &str) -> Self {
        let field: Vec<char> = name.chars().take(MAX_FIELD).collect();
        Self {
            mode,
            dir: normalize("/", dir),
            rows: Vec::new(),
            sel: 0,
            scroll: 0,
            visible: 8,
            caret: field.len(),
            fresh: !field.is_empty(),
            field,
            error: None,
            ask: None,
        }
    }

    /// The folder being shown (absolute).
    pub fn dir(&self) -> &str {
        &self.dir
    }

    pub fn rows(&self) -> &[PickRow] {
        &self.rows
    }

    /// Index of the highlighted row.
    pub fn selected(&self) -> usize {
        self.sel
    }

    /// First visible row.
    pub fn scroll(&self) -> usize {
        self.scroll
    }

    /// The name field's text and caret (in characters).
    pub fn field(&self) -> (String, usize) {
        (self.field.iter().collect(), self.caret)
    }

    /// The name field still holds the name it started with: it is drawn selected, and the
    /// first typed character replaces it.
    pub fn field_fresh(&self) -> bool {
        self.fresh
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// The path the overwrite question is about, while it is open.
    pub fn asking(&self) -> Option<&str> {
        self.ask.as_deref()
    }

    /// How many rows the list shows at once (set from the window height).
    pub fn set_visible(&mut self, rows: usize) {
        self.visible = rows.max(1);
        self.keep_selection_visible();
    }

    /// Show `entries` of folder `dir`: folders first, then files, each sorted
    /// naturally, with `..` on top below the root.
    pub fn set_entries(&mut self, dir: &str, entries: Vec<PickRow>) {
        self.dir = normalize("/", dir);
        let mut rows = entries;
        rows.sort_by(|a, b| {
            b.dir
                .cmp(&a.dir)
                .then_with(|| natural_cmp(a.name.as_bytes(), b.name.as_bytes()))
        });
        if self.dir != "/" {
            rows.insert(
                0,
                PickRow {
                    name: String::from(".."),
                    dir: true,
                    size: 0,
                },
            );
        }
        self.rows = rows;
        self.error = None;
        self.sel = 0;
        self.scroll = 0;
        // Land on the file named in the field, if it is here.
        let name: String = self.field.iter().collect();
        if let Some(i) = self.rows.iter().position(|r| !r.dir && r.name == name) {
            self.sel = i;
            self.keep_selection_visible();
        }
    }

    /// A folder could not be listed: show why, keep the old listing.
    pub fn set_error(&mut self, msg: &str) {
        self.error = Some(String::from(msg));
    }

    /// Forget the error shown (its text was written in the language of the time).
    pub fn clear_error(&mut self) {
        self.error = None;
    }

    /// Ask "replace it?" about `path` (after [`PickEvent::Choose`]).
    pub fn confirm_overwrite(&mut self, path: &str) {
        self.ask = Some(String::from(path));
    }

    fn keep_selection_visible(&mut self) {
        if self.sel < self.scroll {
            self.scroll = self.sel;
        } else if self.sel >= self.scroll + self.visible {
            self.scroll = self.sel + 1 - self.visible;
        }
        let max = self.rows.len().saturating_sub(self.visible);
        self.scroll = self.scroll.min(max);
    }

    fn move_sel(&mut self, delta: isize) {
        if self.rows.is_empty() {
            return;
        }
        let last = self.rows.len() - 1;
        self.sel = if delta >= 0 {
            self.sel.saturating_add(delta as usize).min(last)
        } else {
            self.sel.saturating_sub(delta.unsigned_abs())
        };
        self.keep_selection_visible();
        // Moving onto a file puts its name in the field.
        if let Some(r) = self.rows.get(self.sel)
            && !r.dir
        {
            self.field = r.name.chars().take(MAX_FIELD).collect();
            self.caret = self.field.len();
        }
    }

    /// Scroll the list by `rows` (the mouse wheel) without moving the selection.
    pub fn scroll_by(&mut self, rows: isize) {
        let max = self.rows.len().saturating_sub(self.visible);
        self.scroll = if rows >= 0 {
            self.scroll.saturating_add(rows as usize).min(max)
        } else {
            self.scroll.saturating_sub(rows.unsigned_abs())
        };
    }

    fn row_path(&self, i: usize) -> Option<(String, bool)> {
        let r = self.rows.get(i)?;
        let p = if r.name == ".." {
            normalize(&self.dir, "..")
        } else {
            normalize(&self.dir, &r.name)
        };
        Some((p, r.dir))
    }

    fn activate_row(&mut self, i: usize) -> PickEvent {
        match self.row_path(i) {
            Some((p, true)) => PickEvent::Navigate(p),
            Some((p, false)) => {
                let name = self.rows[i].name.clone();
                self.field = name.chars().take(MAX_FIELD).collect();
                self.caret = self.field.len();
                PickEvent::Choose(p)
            }
            None => PickEvent::None,
        }
    }

    /// A click on list row `i` (index into [`Picker::rows`]); a double click
    /// opens the row.
    pub fn click(&mut self, i: usize, double: bool) -> PickEvent {
        self.fresh = false;
        if i >= self.rows.len() || self.ask.is_some() {
            return PickEvent::None;
        }
        self.sel = i;
        self.keep_selection_visible();
        if double {
            return self.activate_row(i);
        }
        if !self.rows[i].dir {
            self.field = self.rows[i].name.chars().take(MAX_FIELD).collect();
            self.caret = self.field.len();
        }
        PickEvent::Redraw
    }

    fn enter(&mut self) -> PickEvent {
        let typed: String = self.field.iter().collect();
        let typed = typed.trim();
        if typed.is_empty() {
            return self.activate_row(self.sel);
        }
        // A typed name that is a folder here (or ends with `/`) opens it.
        let p = normalize(&self.dir, typed);
        if typed.ends_with('/') || typed == "." || typed == ".." {
            self.field.clear();
            self.caret = 0;
            return PickEvent::Navigate(p);
        }
        if !typed.contains('/')
            && let Some(r) = self.rows.iter().find(|r| r.dir && r.name == typed)
        {
            let p = normalize(&self.dir, &r.name);
            self.field.clear();
            self.caret = 0;
            return PickEvent::Navigate(p);
        }
        PickEvent::Choose(p)
    }

    /// Tab: complete the field from the names in the list.
    fn complete(&mut self) {
        let typed: String = self.field.iter().collect();
        let names: Vec<&str> = self
            .rows
            .iter()
            .filter(|r| r.name != ".." && r.name.starts_with(typed.as_str()))
            .map(|r| r.name.as_str())
            .collect();
        let Some(first) = names.first() else {
            return;
        };
        let mut common: &str = first;
        for n in &names[1..] {
            let k = common
                .chars()
                .zip(n.chars())
                .take_while(|(a, b)| a == b)
                .map(|(a, _)| a.len_utf8())
                .sum::<usize>();
            common = &common[..k];
        }
        if common.chars().count() > self.field.len() {
            self.field = common.chars().take(MAX_FIELD).collect();
            self.caret = self.field.len();
        }
    }

    /// Handle a key.
    pub fn key(&mut self, ev: KeyEvent) -> PickEvent {
        if let Some(path) = self.ask.clone() {
            return match ev.code {
                KeyCode::Enter => {
                    self.ask = None;
                    PickEvent::Overwrite(path)
                }
                KeyCode::Char(c) if !ev.mods.ctrl && matches!(c, 's' | 'S' | 'y' | 'Y') => {
                    self.ask = None;
                    PickEvent::Overwrite(path)
                }
                KeyCode::Esc => {
                    self.ask = None;
                    PickEvent::Redraw
                }
                KeyCode::Char(c) if !ev.mods.ctrl && matches!(c, 'n' | 'N') => {
                    self.ask = None;
                    PickEvent::Redraw
                }
                _ => PickEvent::None,
            };
        }
        self.error = None;
        let (ctrl, alt) = (ev.mods.ctrl, ev.mods.alt);
        if core::mem::take(&mut self.fresh) {
            match ev.code {
                KeyCode::Char(_) if !ctrl && !alt => {
                    self.field.clear();
                    self.caret = 0;
                }
                KeyCode::Backspace | KeyCode::Delete => {
                    self.field.clear();
                    self.caret = 0;
                    return PickEvent::Redraw;
                }
                _ => {}
            }
        }
        match ev.code {
            KeyCode::Esc => PickEvent::Cancel,
            KeyCode::Enter => self.enter(),
            KeyCode::Up => {
                self.move_sel(-1);
                PickEvent::Redraw
            }
            KeyCode::Down => {
                self.move_sel(1);
                PickEvent::Redraw
            }
            KeyCode::PageUp => {
                self.move_sel(-(self.visible as isize));
                PickEvent::Redraw
            }
            KeyCode::PageDown => {
                self.move_sel(self.visible as isize);
                PickEvent::Redraw
            }
            KeyCode::Tab => {
                self.complete();
                PickEvent::Redraw
            }
            KeyCode::Left => {
                self.caret = self.caret.saturating_sub(1);
                PickEvent::Redraw
            }
            KeyCode::Right => {
                self.caret = (self.caret + 1).min(self.field.len());
                PickEvent::Redraw
            }
            KeyCode::Home => {
                self.caret = 0;
                PickEvent::Redraw
            }
            KeyCode::End => {
                self.caret = self.field.len();
                PickEvent::Redraw
            }
            KeyCode::Backspace => {
                if self.caret > 0 {
                    self.caret -= 1;
                    self.field.remove(self.caret);
                } else if self.field.is_empty() && self.dir != "/" {
                    // Backspace on an empty field goes up a folder.
                    return PickEvent::Navigate(normalize(&self.dir, ".."));
                }
                PickEvent::Redraw
            }
            KeyCode::Delete => {
                if self.caret < self.field.len() {
                    self.field.remove(self.caret);
                }
                PickEvent::Redraw
            }
            KeyCode::Char(c) if ctrl && !alt => {
                if c.eq_ignore_ascii_case(&'u') {
                    self.field.clear();
                    self.caret = 0;
                    PickEvent::Redraw
                } else {
                    PickEvent::None
                }
            }
            KeyCode::Char(c) if !alt => {
                if c.is_control() || self.field.len() >= MAX_FIELD {
                    return PickEvent::None;
                }
                self.field.insert(self.caret, c);
                self.caret += 1;
                PickEvent::Redraw
            }
            _ => PickEvent::None,
        }
    }
}

/// The answer to "save changes before closing?".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CloseChoice {
    Save,
    Discard,
    Cancel,
}

/// The three-button question shown when a window with unsaved changes closes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CloseAsk {
    sel: CloseChoice,
}

impl Default for CloseAsk {
    fn default() -> Self {
        Self::new()
    }
}

impl CloseAsk {
    /// Salvar is highlighted first: the safe answer is one Enter away.
    pub const fn new() -> Self {
        Self {
            sel: CloseChoice::Save,
        }
    }

    /// The highlighted button.
    pub const fn selected(&self) -> CloseChoice {
        self.sel
    }

    /// Button label keys, in order; look them up with [`CloseAsk::label`].
    pub const LABELS: [(CloseChoice, &'static str); 3] = [
        (CloseChoice::Save, crate::tk!("edit.save")),
        (CloseChoice::Discard, crate::tk!("edit.close.discard")),
        (CloseChoice::Cancel, crate::tk!("common.cancel")),
    ];

    /// The label of a button in the language in effect.
    pub fn label(choice: CloseChoice) -> &'static str {
        match choice {
            CloseChoice::Save => crate::t!("edit.save"),
            CloseChoice::Discard => crate::t!("edit.close.discard"),
            CloseChoice::Cancel => crate::t!("common.cancel"),
        }
    }

    fn index(&self) -> usize {
        match self.sel {
            CloseChoice::Save => 0,
            CloseChoice::Discard => 1,
            CloseChoice::Cancel => 2,
        }
    }

    fn set_index(&mut self, i: usize) {
        self.sel = Self::LABELS[i % 3].0;
    }

    /// Highlight a button (a mouse hover).
    pub fn select(&mut self, c: CloseChoice) {
        self.sel = c;
    }

    /// Handle a key: `Some(choice)` when the question is answered. S, D and C
    /// (or Esc for cancel) answer at once; arrows and Tab move; Enter takes
    /// the highlighted button.
    pub fn key(&mut self, ev: KeyEvent) -> Option<CloseChoice> {
        match ev.code {
            KeyCode::Esc => Some(CloseChoice::Cancel),
            KeyCode::Enter => Some(self.sel),
            KeyCode::Left => {
                self.set_index(self.index() + 2);
                None
            }
            KeyCode::Right => {
                self.set_index(self.index() + 1);
                None
            }
            KeyCode::Tab => {
                let back = ev.mods.shift;
                self.set_index(self.index() + if back { 2 } else { 1 });
                None
            }
            KeyCode::Char(c) if !ev.mods.ctrl && !ev.mods.alt => match c.to_ascii_lowercase() {
                's' => Some(CloseChoice::Save),
                'd' | 'n' => Some(CloseChoice::Discard),
                'c' => Some(CloseChoice::Cancel),
                _ => None,
            },
            _ => None,
        }
    }
}

/// The status bar text: position, size, encoding, line ending, state (in the language in
/// effect).
pub fn status_line(st: &Status, read_only: bool) -> String {
    let mut s = crate::t!(
        "edit.status.line",
        line = st.line,
        col = st.col,
        lines = &lines_text(st.total_lines),
        size = &crate::apps::fileman::format_size(st.bytes as u64),
        eol = eol_text(st.eol)
    );
    if st.selected_chars > 0 {
        s.push_str("   ");
        s.push_str(&crate::t!("edit.status.sel", n = st.selected_chars));
    }
    if read_only {
        s.push_str("   ");
        s.push_str(crate::t!("edit.status.read_only"));
    } else if st.modified {
        s.push_str("   ");
        s.push_str(crate::t!("edit.status.modified"));
    }
    s
}

fn eol_text(eol: Eol) -> &'static str {
    match eol {
        Eol::Lf => "LF",
        Eol::Crlf => "CRLF",
    }
}

/// `1 linha` / `3 linhas` (`1 line` / `3 lines`).
fn lines_text(n: usize) -> String {
    crate::tp!("edit.status.lines", n)
}

/// The pieces of the editor's status bar, ready to lay out: the cursor position on the left, the
/// document facts on the right.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct StatusBar {
    /// `Ln 12, Col 5`, with the selection size when there is one (`Ln 12, Col 5 (14 selecionados)`).
    pub position: String,
    /// `UTF-8`, `LF` or `CRLF`, the size, the line count, in the order they are drawn.
    pub facts: [String; 4],
    /// The document has unsaved changes.
    pub modified: bool,
}

/// Build the status bar pieces of `st` in the language in effect.
pub fn status_bar(st: &Status) -> StatusBar {
    let mut position = crate::t!("edit.status.pos", line = st.line, col = st.col);
    if st.selected_chars > 0 {
        position.push(' ');
        position.push_str(&crate::tp!("edit.status.selected", st.selected_chars));
    }
    StatusBar {
        position,
        facts: [
            String::from("UTF-8"),
            String::from(eol_text(st.eol)),
            crate::apps::fileman::format_size(st.bytes as u64),
            lines_text(st.total_lines),
        ],
        modified: st.modified,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apps::editor2::Editor;
    use crate::i18n::{Lang, testlang::LangGuard};
    use crate::system::input::Mods;

    fn k(code: KeyCode) -> KeyEvent {
        KeyEvent::plain(code)
    }

    fn row(name: &str, dir: bool) -> PickRow {
        PickRow {
            name: name.into(),
            dir,
            size: 10,
        }
    }

    fn listing() -> Vec<PickRow> {
        alloc::vec![
            row("b.txt", false),
            row("Zeta", true),
            row("a10.txt", false),
            row("a2.txt", false),
            row("alpha", true),
        ]
    }

    fn picker(mode: PickMode, dir: &str, name: &str) -> Picker {
        let mut p = Picker::new(mode, dir, name);
        p.set_entries(dir, listing());
        p
    }

    fn names(p: &Picker) -> Vec<&str> {
        p.rows().iter().map(|r| r.name.as_str()).collect()
    }

    #[test]
    fn listing_is_folders_first_natural_with_parent_row() {
        let p = picker(PickMode::Open, "/docs", "");
        assert_eq!(
            names(&p),
            ["..", "alpha", "Zeta", "a2.txt", "a10.txt", "b.txt"]
        );
        let root = picker(PickMode::Open, "/", "");
        assert_eq!(names(&root)[0], "alpha", "no .. at the root");
        assert_eq!(p.dir(), "/docs");
    }

    #[test]
    fn the_file_named_in_the_field_is_preselected() {
        let p = picker(PickMode::SaveAs, "/", "a10.txt");
        assert_eq!(p.rows()[p.selected()].name, "a10.txt");
        let (text, caret) = p.field();
        assert_eq!((text.as_str(), caret), ("a10.txt", 7));
    }

    #[test]
    fn arrows_select_and_files_fill_the_field() {
        let mut p = picker(PickMode::Open, "/", "");
        assert_eq!(p.key(k(KeyCode::Down)), PickEvent::Redraw); // Zeta
        assert_eq!(p.field().0, "", "folders do not fill the field");
        p.key(k(KeyCode::Down)); // a2.txt
        assert_eq!(p.field().0, "a2.txt");
        p.key(k(KeyCode::PageDown));
        assert_eq!(p.rows()[p.selected()].name, "b.txt");
        assert_eq!(p.field().0, "b.txt");
        p.key(k(KeyCode::Down));
        assert_eq!(p.rows()[p.selected()].name, "b.txt", "stops at the end");
        p.key(k(KeyCode::PageUp));
        assert_eq!(p.selected(), 0);
    }

    #[test]
    fn enter_opens_folders_and_chooses_files() {
        let mut p = picker(PickMode::Open, "/docs", "");
        // ".." is selected first: Enter goes up.
        assert_eq!(p.key(k(KeyCode::Enter)), PickEvent::Navigate("/".into()));
        p.key(k(KeyCode::Down)); // alpha
        assert_eq!(
            p.key(k(KeyCode::Enter)),
            PickEvent::Navigate("/docs/alpha".into())
        );
        p.key(k(KeyCode::Down));
        p.key(k(KeyCode::Down)); // Zeta, a2.txt
        assert_eq!(
            p.key(k(KeyCode::Enter)),
            PickEvent::Choose("/docs/a2.txt".into())
        );
    }

    #[test]
    fn typed_names_are_resolved_against_the_folder() {
        let mut p = picker(PickMode::SaveAs, "/docs", "");
        for c in "new.txt".chars() {
            p.key(KeyEvent::ch(c));
        }
        assert_eq!(
            p.key(k(KeyCode::Enter)),
            PickEvent::Choose("/docs/new.txt".into())
        );
        let mut p = picker(PickMode::SaveAs, "/docs", "");
        for c in "../x/y.txt".chars() {
            p.key(KeyEvent::ch(c));
        }
        assert_eq!(
            p.key(k(KeyCode::Enter)),
            PickEvent::Choose("/x/y.txt".into())
        );
        let mut p = picker(PickMode::Open, "/docs", "");
        for c in "/etc/hosts".chars() {
            p.key(KeyEvent::ch(c));
        }
        assert_eq!(
            p.key(k(KeyCode::Enter)),
            PickEvent::Choose("/etc/hosts".into())
        );
    }

    #[test]
    fn a_typed_folder_name_navigates() {
        let mut p = picker(PickMode::Open, "/", "");
        for c in "alpha".chars() {
            p.key(KeyEvent::ch(c));
        }
        assert_eq!(
            p.key(k(KeyCode::Enter)),
            PickEvent::Navigate("/alpha".into())
        );
        assert_eq!(p.field().0, "", "the field is cleared for the next name");
        let mut p = picker(PickMode::Open, "/", "");
        for c in "sub/".chars() {
            p.key(KeyEvent::ch(c));
        }
        assert_eq!(p.key(k(KeyCode::Enter)), PickEvent::Navigate("/sub".into()));
    }

    #[test]
    fn backspace_on_an_empty_field_goes_up_but_never_above_the_root() {
        let mut p = picker(PickMode::Open, "/a/b", "");
        assert_eq!(
            p.key(k(KeyCode::Backspace)),
            PickEvent::Navigate("/a".into())
        );
        let mut p = picker(PickMode::Open, "/", "");
        assert_eq!(p.key(k(KeyCode::Backspace)), PickEvent::Redraw);
    }

    #[test]
    fn field_editing_is_character_aware() {
        let mut p = picker(PickMode::SaveAs, "/", "");
        for c in "açãe".chars() {
            p.key(KeyEvent::ch(c));
        }
        p.key(k(KeyCode::Left));
        p.key(k(KeyCode::Backspace));
        assert_eq!(p.field(), ("açe".into(), 2));
        p.key(k(KeyCode::Home));
        p.key(k(KeyCode::Delete));
        assert_eq!(p.field().0, "çe");
        p.key(k(KeyCode::End));
        p.key(KeyEvent::ctrl('u'));
        assert_eq!(p.field(), (String::new(), 0));
        // Control characters and Alt chords are not text; the field is capped.
        p.key(KeyEvent::ch('\u{7}'));
        p.key(KeyEvent::new(KeyCode::Char('x'), Mods::ALT));
        assert_eq!(p.field().0, "");
        for _ in 0..(MAX_FIELD + 20) {
            p.key(KeyEvent::ch('z'));
        }
        assert_eq!(p.field().0.len(), MAX_FIELD);
    }

    #[test]
    fn the_first_typed_character_replaces_the_starting_name() {
        let mut p = picker(PickMode::SaveAs, "/", "sem-nome.txt");
        p.key(KeyEvent::ch('n'));
        p.key(KeyEvent::ch('o'));
        assert_eq!(p.field().0, "no");
        // Backspace clears the whole starting name, once.
        let mut p = picker(PickMode::SaveAs, "/", "sem-nome.txt");
        p.key(k(KeyCode::Backspace));
        assert_eq!(p.field().0, "");
        p.key(KeyEvent::ch('a'));
        p.key(k(KeyCode::Backspace));
        assert_eq!(p.field().0, "");
        // Moving the caret first keeps the name for editing.
        let mut p = picker(PickMode::SaveAs, "/", "sem-nome.txt");
        p.key(k(KeyCode::End));
        p.key(KeyEvent::ch('2'));
        assert_eq!(p.field().0, "sem-nome.txt2");
        // Choosing with Enter keeps it as typed.
        let mut p = picker(PickMode::SaveAs, "/", "sem-nome.txt");
        assert_eq!(
            p.key(k(KeyCode::Enter)),
            PickEvent::Choose("/sem-nome.txt".into())
        );
    }

    #[test]
    fn tab_completes_the_longest_common_prefix() {
        let mut p = picker(PickMode::Open, "/", "");
        p.key(KeyEvent::ch('a'));
        p.key(k(KeyCode::Tab));
        assert_eq!(p.field().0, "a", "alpha, a2.txt, a10.txt share only 'a'");
        p.key(KeyEvent::ch('l'));
        p.key(k(KeyCode::Tab));
        assert_eq!(p.field().0, "alpha");
        let mut p = picker(PickMode::Open, "/", "b");
        p.key(k(KeyCode::Tab));
        assert_eq!(p.field().0, "b.txt");
        let mut p = picker(PickMode::Open, "/", "q");
        p.key(k(KeyCode::Tab));
        assert_eq!(p.field().0, "q", "no match leaves the text alone");
    }

    #[test]
    fn mouse_clicks_select_and_double_clicks_open() {
        let mut p = picker(PickMode::Open, "/", "");
        assert_eq!(p.click(1, false), PickEvent::Redraw);
        assert_eq!(p.selected(), 1);
        assert_eq!(p.click(1, true), PickEvent::Navigate("/Zeta".into()));
        assert_eq!(p.click(2, false), PickEvent::Redraw);
        assert_eq!(p.field().0, "a2.txt");
        assert_eq!(p.click(2, true), PickEvent::Choose("/a2.txt".into()));
        assert_eq!(p.click(99, true), PickEvent::None);
    }

    #[test]
    fn the_list_scrolls_with_the_selection_and_the_wheel() {
        let mut p = Picker::new(PickMode::Open, "/", "");
        let many: Vec<PickRow> = (0..30)
            .map(|i| row(&alloc::format!("f{i:02}"), false))
            .collect();
        p.set_entries("/", many);
        p.set_visible(5);
        for _ in 0..12 {
            p.key(k(KeyCode::Down));
        }
        assert_eq!(p.selected(), 12);
        assert_eq!(p.scroll(), 8);
        p.scroll_by(-100);
        assert_eq!(p.scroll(), 0);
        assert_eq!(p.selected(), 12, "the wheel leaves the selection alone");
        p.scroll_by(1000);
        assert_eq!(p.scroll(), 25);
        // Shrinking the window keeps the selection in view.
        p.set_visible(3);
        assert!(p.selected() >= p.scroll() && p.selected() < p.scroll() + 3);
    }

    #[test]
    fn errors_show_until_the_next_key() {
        let mut p = picker(PickMode::Open, "/", "");
        p.set_error("Pasta não encontrada");
        assert_eq!(p.error(), Some("Pasta não encontrada"));
        p.key(KeyEvent::ch('x'));
        assert_eq!(p.error(), None);
        // A listing that fails leaves the old rows in place.
        let before = names(&p).len();
        p.set_error("x");
        assert_eq!(names(&p).len(), before);
    }

    #[test]
    fn the_overwrite_question_takes_over_the_keys() {
        let mut p = picker(PickMode::SaveAs, "/", "a2.txt");
        assert_eq!(
            p.key(k(KeyCode::Enter)),
            PickEvent::Choose("/a2.txt".into())
        );
        p.confirm_overwrite("/a2.txt");
        assert_eq!(p.asking(), Some("/a2.txt"));
        // Typing is ignored while the question is open.
        assert_eq!(p.key(KeyEvent::ch('q')), PickEvent::None);
        assert_eq!(p.click(1, true), PickEvent::None);
        assert_eq!(
            p.key(k(KeyCode::Esc)),
            PickEvent::Redraw,
            "Esc only closes the question"
        );
        assert_eq!(p.asking(), None);
        p.confirm_overwrite("/a2.txt");
        assert_eq!(p.key(KeyEvent::ch('n')), PickEvent::Redraw);
        p.confirm_overwrite("/a2.txt");
        assert_eq!(
            p.key(k(KeyCode::Enter)),
            PickEvent::Overwrite("/a2.txt".into())
        );
        p.confirm_overwrite("/a2.txt");
        assert_eq!(
            p.key(KeyEvent::ch('s')),
            PickEvent::Overwrite("/a2.txt".into())
        );
        assert_eq!(
            p.key(k(KeyCode::Esc)),
            PickEvent::Cancel,
            "now Esc cancels the dialog"
        );
    }

    #[test]
    fn close_question_answers() {
        let mut a = CloseAsk::new();
        assert_eq!(a.selected(), CloseChoice::Save);
        assert_eq!(a.key(k(KeyCode::Enter)), Some(CloseChoice::Save));
        assert_eq!(a.key(k(KeyCode::Right)), None);
        assert_eq!(a.selected(), CloseChoice::Discard);
        assert_eq!(a.key(k(KeyCode::Enter)), Some(CloseChoice::Discard));
        a.key(k(KeyCode::Tab));
        assert_eq!(a.selected(), CloseChoice::Cancel);
        a.key(k(KeyCode::Tab));
        assert_eq!(a.selected(), CloseChoice::Save, "wraps around");
        a.key(k(KeyCode::Left));
        assert_eq!(a.selected(), CloseChoice::Cancel);
        a.key(KeyEvent::plain(KeyCode::Tab).shifted());
        assert_eq!(a.selected(), CloseChoice::Discard);
        assert_eq!(a.key(KeyEvent::ch('S')), Some(CloseChoice::Save));
        assert_eq!(a.key(KeyEvent::ch('d')), Some(CloseChoice::Discard));
        assert_eq!(a.key(KeyEvent::ch('c')), Some(CloseChoice::Cancel));
        assert_eq!(a.key(k(KeyCode::Esc)), Some(CloseChoice::Cancel));
        assert_eq!(a.key(KeyEvent::ch('x')), None);
        assert_eq!(a.key(KeyEvent::ctrl('s')), None, "chords are not answers");
        a.select(CloseChoice::Cancel);
        assert_eq!(a.key(k(KeyCode::Enter)), Some(CloseChoice::Cancel));
    }

    #[test]
    fn status_bar_pieces_follow_the_editor() {
        let _g = LangGuard::new(Lang::Pt);
        let mut e = Editor::from_bytes(b"ola\nmundo\n");
        e.set_cursor(1, 2);
        let b = status_bar(&e.status());
        assert_eq!(b.position, "Ln 2, Col 3");
        assert_eq!(b.facts[0], "UTF-8");
        assert_eq!(b.facts[1], "LF");
        assert_eq!(b.facts[2], "10 B");
        assert_eq!(b.facts[3], "3 linhas");
        assert!(!b.modified);
        let big = status_bar(&Editor::from_bytes(&alloc::vec![b'x'; 1536]).status());
        assert_eq!(big.facts[2], "1,5 KiB");
        e.select_all();
        let b = status_bar(&e.status());
        assert!(b.position.ends_with("(10 selecionados)"), "{}", b.position);
        let one = status_bar(&Editor::from_bytes(b"x").status());
        assert_eq!(one.facts[3], "1 linha");
        let mut crlf = Editor::from_bytes(b"a\r\nb\r\n");
        crlf.insert_char('z');
        let b = status_bar(&crlf.status());
        assert_eq!(b.facts[1], "CRLF");
        assert!(b.modified);
        let mut sel1 = Editor::from_bytes(b"ab");
        sel1.select_range(0, 1);
        assert!(
            status_bar(&sel1.status())
                .position
                .ends_with("(1 selecionado)")
        );
    }

    #[test]
    fn status_line_reports_position_size_and_state() {
        let _g = LangGuard::new(Lang::Pt);
        let mut e = Editor::from_bytes(b"one\r\ntwo\r\nthree");
        e.set_cursor(1, 2);
        let s = status_line(&e.status(), false);
        assert!(
            s.starts_with("Ln 2, Col 3   3 linhas   15 B   UTF-8   CRLF"),
            "{s}"
        );
        assert!(!s.contains("modificado"));
        e.insert_str("x");
        let s = status_line(&e.status(), false);
        assert!(s.ends_with("* modificado"), "{s}");
        assert!(status_line(&e.status(), true).ends_with("SOMENTE LEITURA"));
        e.select_all();
        assert!(status_line(&e.status(), false).contains("sel 16"));
        let big = Editor::from_bytes(&alloc::vec![b'a'; 1_572_864]);
        assert!(status_line(&big.status(), false).contains("1,5 MiB"));
    }

    #[test]
    fn status_texts_in_english() {
        let _g = LangGuard::new(Lang::En);
        let mut e = Editor::from_bytes(b"one\r\ntwo\r\nthree");
        e.set_cursor(1, 2);
        let s = status_line(&e.status(), false);
        assert!(
            s.starts_with("Ln 2, Col 3   3 lines   15 B   UTF-8   CRLF"),
            "{s}"
        );
        e.insert_str("x");
        assert!(status_line(&e.status(), false).ends_with("* modified"));
        assert!(status_line(&e.status(), true).ends_with("READ ONLY"));
        let b = status_bar(&Editor::from_bytes(b"x").status());
        assert_eq!(b.facts[3], "1 line");
        let big = status_bar(&Editor::from_bytes(&alloc::vec![b'x'; 1536]).status());
        assert_eq!(big.facts[2], "1.5 KiB");
        let mut e = Editor::from_bytes(b"ab");
        e.select_range(0, 2);
        assert!(status_bar(&e.status()).position.ends_with("(2 selected)"));
        assert_eq!(CloseAsk::label(CloseChoice::Discard), "Discard");
        assert_eq!(CloseAsk::label(CloseChoice::Cancel), "Cancel");
    }

    #[test]
    fn close_labels_in_portuguese() {
        let _g = LangGuard::new(Lang::Pt);
        assert_eq!(CloseAsk::label(CloseChoice::Save), "Salvar");
        assert_eq!(CloseAsk::label(CloseChoice::Discard), "Descartar");
        assert_eq!(CloseAsk::label(CloseChoice::Cancel), "Cancelar");
    }

    #[test]
    fn the_starting_name_is_fresh_until_a_key_touches_it() {
        let mut p = Picker::new(PickMode::SaveAs, "/", "nota.txt");
        assert!(p.field_fresh());
        p.key(k(KeyCode::Char('x')));
        assert!(!p.field_fresh());
        assert_eq!(p.field().0, "x");
        assert!(!Picker::new(PickMode::Open, "/", "").field_fresh());
    }
}
