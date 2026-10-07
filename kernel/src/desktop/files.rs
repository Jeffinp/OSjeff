//! `Desktop` methods: fs. Split out of the former monolithic desktop.rs.
//!
//! Filesystem actions act on app *instances*: terminal commands print into the
//! terminal window that issued them, and editor buffers are per editor window.

use super::*;

/// Serialize an editor buffer (lines joined by `\n`) into `out`. Returns the
/// byte count written (capped at `out.len()`).
fn serialize_editor(editor: &Editor, out: &mut [u8]) -> usize {
    let mut n = 0;
    let rows = editor.rows();
    for i in 0..rows {
        for &b in editor.line(i) {
            if n < out.len() {
                out[n] = b;
                n += 1;
            }
        }
        if i + 1 < rows && n < out.len() {
            out[n] = b'\n';
            n += 1;
        }
    }
    n
}

impl Desktop {
    /// Print `text` into terminal window `tid`, or — when `None` (the action
    /// came from an editor or the file manager) — into the most recently used
    /// terminal, if there is one.
    pub(crate) fn say(&mut self, tid: Option<WindowId>, text: &[u8]) {
        let target = tid.or_else(|| self.mru_of_kind(Kind::Terminal));
        if let Some(t) = target.and_then(|id| self.term_mut(id)) {
            t.println(text);
        }
    }

    /// Ctrl+S: save the focused editor's buffer to its current file, in the
    /// folder it was opened from.
    pub(crate) fn save_editor_file(&mut self) {
        let Some(top) = self.focused() else {
            return;
        };
        let Some((dir, file)) = self.editor_mut(top).map(|e| (e.dir, e.file)) else {
            return;
        };
        self.fs_save_in(top, dir, file, None);
    }

    /// Terminal `save <name>`: saves the most recently used editor's buffer as a
    /// top-level file (the shell has no current directory).
    pub(crate) fn fs_save(&mut self, tid: WindowId, f: FileName) {
        match self.mru_of_kind(Kind::Editor) {
            Some(eid) => self.fs_save_in(eid, fs::ROOT, f, Some(tid)),
            None => self.say(Some(tid), b"no editor open"),
        }
    }

    /// Write editor window `eid`'s buffer to file `f` inside directory `dir`. If
    /// `dir` was trashed or deleted since the file was opened, fall back to the
    /// root rather than writing under a stale slot.
    pub(crate) fn fs_save_in(
        &mut self,
        eid: WindowId,
        dir: u8,
        f: FileName,
        tid: Option<WindowId>,
    ) {
        let mut buf = [0u8; fs::MAX_FILE_SIZE];
        let n;
        {
            let Some(e) = self.editor_mut(eid) else {
                return;
            };
            // The file did not fit the editor grid when it was opened, so the buffer
            // holds only part of it: writing it back would silently destroy the rest.
            if e.editor.is_lossy() {
                self.say(tid, b"not saved: file is larger than the editor window");
                crate::serial_println!("editor: refusing to save a truncated buffer");
                return;
            }
            n = serialize_editor(&e.editor, &mut buf);
        }
        let dir = fs::live_dir(disk(), dir);
        match fs::write_in(disk(), dir, f.as_bytes(), &buf[..n]) {
            Ok(()) => {
                flush_disk();
                if let Some(e) = self.editor_mut(eid) {
                    e.editor.mark_clean();
                    e.file = f;
                    e.dir = dir;
                }
                self.print_named(tid, b"Saved ", f.as_bytes());
            }
            Err(e) => self.print_fs_err(tid, e),
        }
    }

    /// Terminal `load <name>`: a top-level file.
    pub(crate) fn fs_load(&mut self, tid: WindowId, f: FileName) {
        self.fs_load_in(Some(tid), fs::ROOT, f);
    }

    /// Open file `f` of directory `dir` in an editor window: the one that
    /// already shows it, else a new one.
    pub(crate) fn fs_load_in(&mut self, tid: Option<WindowId>, dir: u8, f: FileName) {
        // Copy the file out of the static disk before touching any editor.
        let mut buf = [0u8; fs::MAX_FILE_SIZE];
        let found = fs::read_in(disk(), dir, f.as_bytes()).map(|data| {
            let n = data.len();
            buf[..n].copy_from_slice(data);
            n
        });
        let Some(n) = found else {
            self.say(tid, b"file not found");
            return;
        };
        // Already open (and possibly edited): just bring it forward.
        let open = self
            .wm
            .windows()
            .iter()
            .filter(|w| !w.is_closing())
            .find_map(|w| match &w.app.app {
                App::Editor(e) if e.file == f && e.dir == dir => Some(w.id),
                _ => None,
            });
        if let Some(id) = open {
            self.wm.activate(id);
            self.print_named(tid, b"Loaded ", f.as_bytes());
            return;
        }
        let Some(id) = self.open_new(Kind::Editor) else {
            self.say(tid, b"error: too many windows");
            return;
        };
        if let Some(e) = self.editor_mut(id) {
            e.editor.set_text(&buf[..n]);
            e.file = f;
            e.dir = dir;
        }
        self.print_named(tid, b"Loaded ", f.as_bytes());
    }

    /// Open the file stored in slot `slot` (the file manager's selection),
    /// remembering which folder it came from.
    pub(crate) fn fs_load_slot(&mut self, slot: usize) {
        let img = disk();
        if fs::is_dir(img, slot) {
            return;
        }
        let dir = fs::parent_at(img, slot);
        if let Some(f) = FileName::parse(fs::name_at(img, slot)) {
            self.fs_load_in(None, dir, f);
        }
    }

    pub(crate) fn fs_cat(&mut self, tid: WindowId, f: FileName) {
        let data = match fs::read(disk(), f.as_bytes()) {
            Some(d) => d,
            None => {
                self.say(Some(tid), b"file not found");
                return;
            }
        };
        if data.is_empty() {
            self.say(Some(tid), b"(empty)");
            return;
        }
        let mut start = 0;
        for i in 0..data.len() {
            if data[i] == b'\n' {
                self.say(Some(tid), &data[start..i]);
                start = i + 1;
            }
        }
        if start < data.len() {
            self.say(Some(tid), &data[start..]);
        }
    }

    pub(crate) fn fs_remove(&mut self, tid: WindowId, f: FileName) {
        match fs::remove(disk(), f.as_bytes()) {
            Ok(()) => {
                flush_disk();
                self.print_named(Some(tid), b"Removed ", f.as_bytes());
            }
            Err(e) => self.print_fs_err(Some(tid), e),
        }
    }

    pub(crate) fn fs_list(&mut self, tid: WindowId) {
        let d = disk();
        if fs::count_active(d) == 0 {
            self.say(Some(tid), b"(no files)");
            return;
        }
        for i in 0..fs::MAX_FILES {
            // Trashed files are hidden from LIST (see the file manager's Trash).
            if !fs::is_active(d, i) {
                continue;
            }
            let mut line = [b' '; 28];
            let name = fs::name_at(d, i);
            line[..name.len()].copy_from_slice(name);
            write_uint(&mut line, 18, 6, fs::size_at(d, i) as u32);
            self.say(Some(tid), &line);
        }
    }

    pub(crate) fn print_named(&mut self, tid: Option<WindowId>, prefix: &[u8], name: &[u8]) {
        let mut line = [b' '; 40];
        let mut p = 0;
        for &b in prefix.iter().chain(name.iter()) {
            if p < line.len() {
                line[p] = b;
                p += 1;
            }
        }
        self.say(tid, &line[..p]);
    }

    pub(crate) fn print_fs_err(&mut self, tid: Option<WindowId>, e: fs::FsError) {
        let msg: &[u8] = match e {
            fs::FsError::NoSpace => b"error: disk full",
            fs::FsError::TooBig => b"error: file too big",
            fs::FsError::NameTooLong => b"error: name too long",
            fs::FsError::NotFound => b"error: not found",
            fs::FsError::EmptyName => b"error: empty name",
            fs::FsError::NotFormatted => b"error: no filesystem",
        };
        self.say(tid, msg);
    }
}
