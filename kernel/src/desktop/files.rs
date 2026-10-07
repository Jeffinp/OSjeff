//! `Desktop` methods: fs. Split out of the former monolithic desktop.rs.

use super::*;

impl Desktop {
    /// Serialize the editor buffer (lines joined by `\n`) into `out`. Returns the
    /// byte count written (capped at `out.len()`).
    pub(crate) fn serialize_editor(&self, out: &mut [u8]) -> usize {
        let mut n = 0;
        let rows = self.editor.rows();
        for i in 0..rows {
            for &b in self.editor.line(i) {
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

    /// Ctrl+S: save the editor buffer to its current file, in the folder it was
    /// opened from.
    pub(crate) fn save_editor_file(&mut self) {
        if self.focused().map(|w| self.windows[w].kind) != Some(Kind::Editor) {
            return;
        }
        self.fs_save_in(self.editor_dir, self.editor_file);
    }

    /// Terminal `save <name>`: the shell has no current directory, so this is a
    /// top-level file.
    pub(crate) fn fs_save(&mut self, f: FileName) {
        self.fs_save_in(fs::ROOT, f);
    }

    /// Write the editor buffer to file `f` inside directory `dir`. If `dir` was
    /// trashed or deleted since the file was opened, fall back to the root rather
    /// than writing under a stale slot.
    pub(crate) fn fs_save_in(&mut self, dir: u8, f: FileName) {
        // The file did not fit the editor grid when it was opened, so the buffer
        // holds only part of it: writing it back would silently destroy the rest.
        if self.editor.is_lossy() {
            self.term
                .println(b"not saved: file is larger than the editor window");
            crate::serial_println!("editor: refusing to save a truncated buffer");
            return;
        }
        let mut buf = [0u8; fs::MAX_FILE_SIZE];
        let n = self.serialize_editor(&mut buf);
        let dir = fs::live_dir(disk(), dir);
        match fs::write_in(disk(), dir, f.as_bytes(), &buf[..n]) {
            Ok(()) => {
                flush_disk();
                self.editor.mark_clean();
                self.editor_file = f;
                self.editor_dir = dir;
                self.print_named(b"Saved ", f.as_bytes());
            }
            Err(e) => self.print_fs_err(e),
        }
    }

    /// Terminal `load <name>`: a top-level file.
    pub(crate) fn fs_load(&mut self, f: FileName) {
        self.fs_load_in(fs::ROOT, f);
    }

    /// Open file `f` of directory `dir` in the editor.
    pub(crate) fn fs_load_in(&mut self, dir: u8, f: FileName) {
        // Copy the file out of the static disk before touching the editor.
        let mut buf = [0u8; fs::MAX_FILE_SIZE];
        let found = fs::read_in(disk(), dir, f.as_bytes()).map(|data| {
            let n = data.len();
            buf[..n].copy_from_slice(data);
            n
        });
        match found {
            Some(n) => {
                self.editor.set_text(&buf[..n]);
                self.editor_file = f;
                self.editor_dir = dir;
                self.open(EDIT);
                self.print_named(b"Loaded ", f.as_bytes());
            }
            None => self.term.println(b"file not found"),
        }
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
            self.fs_load_in(dir, f);
        }
    }

    pub(crate) fn fs_cat(&mut self, f: FileName) {
        let data = match fs::read(disk(), f.as_bytes()) {
            Some(d) => d,
            None => {
                self.term.println(b"file not found");
                return;
            }
        };
        if data.is_empty() {
            self.term.println(b"(empty)");
            return;
        }
        let mut start = 0;
        for i in 0..data.len() {
            if data[i] == b'\n' {
                self.term.println(&data[start..i]);
                start = i + 1;
            }
        }
        if start < data.len() {
            self.term.println(&data[start..]);
        }
    }

    pub(crate) fn fs_remove(&mut self, f: FileName) {
        match fs::remove(disk(), f.as_bytes()) {
            Ok(()) => {
                flush_disk();
                self.print_named(b"Removed ", f.as_bytes());
            }
            Err(e) => self.print_fs_err(e),
        }
    }

    pub(crate) fn fs_list(&mut self) {
        let d = disk();
        if fs::count_active(d) == 0 {
            self.term.println(b"(no files)");
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
            self.term.println(&line);
        }
    }

    pub(crate) fn print_named(&mut self, prefix: &[u8], name: &[u8]) {
        let mut line = [b' '; 40];
        let mut p = 0;
        for &b in prefix.iter().chain(name.iter()) {
            if p < line.len() {
                line[p] = b;
                p += 1;
            }
        }
        self.term.println(&line[..p]);
    }

    pub(crate) fn print_fs_err(&mut self, e: fs::FsError) {
        let msg: &[u8] = match e {
            fs::FsError::NoSpace => b"error: disk full",
            fs::FsError::TooBig => b"error: file too big",
            fs::FsError::NameTooLong => b"error: name too long",
            fs::FsError::NotFound => b"error: not found",
            fs::FsError::EmptyName => b"error: empty name",
            fs::FsError::NotFormatted => b"error: no filesystem",
        };
        self.term.println(msg);
    }
}
