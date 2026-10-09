//! A safe, allocation-free reader for the section table of a WebAssembly binary.
//!
//! This is the only code that looks at an *untrusted* `.wasm` before the
//! interpreter does, so it is deliberately small and strict: it checks the
//! 8-byte header, walks the sections (`id`, LEB128 size, payload) and returns
//! **slices of the caller's buffer**. It never allocates, never indexes out of
//! range and never panics, whatever the input (see the `app_manifest` fuzz
//! target and the tests).
//!
//! Limits: a section size that does not fit in what is left of the buffer is an
//! error; LEB128 integers have at most 5 bytes (and the fifth may carry only 4
//! payload bits, as the spec requires of a `u32`); at most [`MAX_SECTIONS`]
//! sections are walked; a custom section's name must lie inside its payload.
//! The reader does **not** validate the module (the interpreter does that); it
//! only finds metadata sections such as `kitsune.manifest`.

use core::fmt;

/// Module header: `\0asm` followed by version 1 (little endian).
pub const HEADER: [u8; 8] = [0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00];
/// Most sections a walk will visit (real modules have at most ~15 non-custom
/// sections plus a handful of custom ones; this bounds a hostile flood).
pub const MAX_SECTIONS: usize = 4096;
/// Highest non-custom section id the spec defines (`tag` = 13).
pub const MAX_SECTION_ID: u8 = 13;

/// Why a binary could not be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WasmError {
    /// Fewer than 8 bytes.
    TooShort,
    /// Not `\0asm`.
    BadMagic,
    /// Version is not 1.
    BadVersion,
    /// A LEB128 integer is unterminated, longer than 5 bytes, or exceeds 32 bits.
    BadLeb,
    /// A section (or its name) extends past the end of the buffer.
    Truncated,
    /// A section id above 13.
    BadSectionId,
    /// More than [`MAX_SECTIONS`] sections.
    TooManySections,
}

impl WasmError {
    /// Catalog key of the reason, in words for the person installing the app.
    pub fn key(self) -> &'static str {
        match self {
            WasmError::TooShort => crate::tk!("apps.err.wasm_short"),
            WasmError::BadMagic => crate::tk!("apps.err.wasm_magic"),
            WasmError::BadVersion => crate::tk!("apps.err.wasm_version"),
            WasmError::BadLeb => crate::tk!("apps.err.wasm_leb"),
            WasmError::Truncated => crate::tk!("apps.err.wasm_section_end"),
            WasmError::BadSectionId => crate::tk!("apps.err.wasm_section_id"),
            WasmError::TooManySections => crate::tk!("apps.err.wasm_sections"),
        }
    }
}

impl fmt::Display for WasmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            WasmError::TooShort => "not a wasm module (too short)",
            WasmError::BadMagic => "not a wasm module (bad magic)",
            WasmError::BadVersion => "unsupported wasm version",
            WasmError::BadLeb => "malformed LEB128 integer",
            WasmError::Truncated => "section extends past the end of the file",
            WasmError::BadSectionId => "unknown section id",
            WasmError::TooManySections => "too many sections",
        })
    }
}

/// Reads an unsigned LEB128 `u32` at `buf[pos..]`; returns the value and the
/// position after it. At most 5 bytes; the fifth must be `<= 0x0F` with no
/// continuation bit. Never panics.
pub fn read_leb_u32(buf: &[u8], pos: usize) -> Result<(u32, usize), WasmError> {
    let mut value: u32 = 0;
    for i in 0..5usize {
        let at = pos.checked_add(i).ok_or(WasmError::BadLeb)?;
        let b = *buf.get(at).ok_or(WasmError::BadLeb)?;
        if i == 4 && b > 0x0F {
            return Err(WasmError::BadLeb);
        }
        value |= ((b & 0x7F) as u32) << (7 * i);
        if b & 0x80 == 0 {
            return Ok((value, at + 1));
        }
    }
    Err(WasmError::BadLeb)
}

/// One section: its id and payload, plus the name when it is a custom section.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Section<'a> {
    /// Section id (0 = custom).
    pub id: u8,
    /// Custom section name (bytes, not necessarily UTF-8); empty for other ids.
    pub name: &'a [u8],
    /// Payload: the whole section for ids != 0, the bytes after the name for custom ones.
    pub data: &'a [u8],
}

/// Iterator over the sections of a module. Yields `Err` once and then stops.
#[derive(Clone, Debug)]
pub struct Sections<'a> {
    buf: &'a [u8],
    pos: usize,
    count: usize,
    failed: bool,
}

impl<'a> Sections<'a> {
    /// Checks the header and positions the walk at the first section.
    pub fn new(buf: &'a [u8]) -> Result<Sections<'a>, WasmError> {
        if buf.len() < 8 {
            return Err(WasmError::TooShort);
        }
        if buf[..4] != HEADER[..4] {
            return Err(WasmError::BadMagic);
        }
        if buf[4..8] != HEADER[4..8] {
            return Err(WasmError::BadVersion);
        }
        Ok(Sections {
            buf,
            pos: 8,
            count: 0,
            failed: false,
        })
    }

    fn step(&mut self) -> Result<Option<Section<'a>>, WasmError> {
        if self.pos >= self.buf.len() {
            return Ok(None);
        }
        if self.count >= MAX_SECTIONS {
            return Err(WasmError::TooManySections);
        }
        self.count += 1;
        let id = self.buf[self.pos];
        if id > MAX_SECTION_ID {
            return Err(WasmError::BadSectionId);
        }
        let (size, after) = read_leb_u32(self.buf, self.pos + 1)?;
        let end = after
            .checked_add(size as usize)
            .ok_or(WasmError::Truncated)?;
        if end > self.buf.len() {
            return Err(WasmError::Truncated);
        }
        let payload = &self.buf[after..end];
        self.pos = end;
        if id != 0 {
            return Ok(Some(Section {
                id,
                name: &[],
                data: payload,
            }));
        }
        let (nlen, nafter) = read_leb_u32(payload, 0)?;
        let nend = nafter
            .checked_add(nlen as usize)
            .ok_or(WasmError::Truncated)?;
        if nend > payload.len() {
            return Err(WasmError::Truncated);
        }
        Ok(Some(Section {
            id: 0,
            name: &payload[nafter..nend],
            data: &payload[nend..],
        }))
    }
}

impl<'a> Iterator for Sections<'a> {
    type Item = Result<Section<'a>, WasmError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed {
            return None;
        }
        match self.step() {
            Ok(Some(s)) => Some(Ok(s)),
            Ok(None) => None,
            Err(e) => {
                self.failed = true;
                Some(Err(e))
            }
        }
    }
}

/// How many custom sections are named `name`, and the payload of the first.
/// Walks the whole module, so a malformed tail is reported even when the
/// section was found.
pub fn find_custom<'a>(
    wasm: &'a [u8],
    name: &[u8],
) -> Result<(usize, Option<&'a [u8]>), WasmError> {
    let mut n = 0;
    let mut first = None;
    for s in Sections::new(wasm)? {
        let s = s?;
        if s.id == 0 && s.name == name {
            n += 1;
            if first.is_none() {
                first = Some(s.data);
            }
        }
    }
    Ok((n, first))
}

#[cfg(test)]
mod tests;
