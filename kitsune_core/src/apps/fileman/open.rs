//! open (split out of `fileman.rs`).

use super::*;

/// How a file opens.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileClass {
    /// PNG, BMP, PPM: the image viewer.
    Image,
    /// WebAssembly module: the apps platform.
    Wasm,
    /// Opens in the editor.
    Text,
    /// Unknown: the editor if the content looks like text.
    Other,
}

/// The lowercase extension of `name` (without the dot), if any.
pub fn extension(name: &[u8]) -> Vec<u8> {
    let (_, ext) = vfs::split_ext(name);
    ext.iter().skip(1).map(u8::to_ascii_lowercase).collect()
}

/// Classify a file by its name.
pub fn classify(name: &[u8]) -> FileClass {
    match &extension(name)[..] {
        b"png" | b"bmp" | b"ppm" => FileClass::Image,
        b"wasm" => FileClass::Wasm,
        b"txt" | b"md" | b"rs" | b"c" | b"h" | b"toml" | b"json" | b"log" | b"ini" | b"cfg"
        | b"csv" | b"html" | b"htm" | b"css" | b"js" | b"sh" | b"py" | b"yml" | b"yaml"
        | b"xml" | b"lock" | b"conf" | b"" => FileClass::Text,
        _ => FileClass::Other,
    }
}

/// Whether `head` (the first bytes of a file) looks like text: no NUL, mostly
/// printable or whitespace.
pub fn looks_like_text(head: &[u8]) -> bool {
    if head.contains(&0) {
        return false;
    }
    let odd = head
        .iter()
        .filter(|&&b| b < 0x20 && !matches!(b, b'\n' | b'\r' | b'\t'))
        .count();
    odd * 20 <= head.len()
}

/// Whether an image viewer can open `name`.
pub fn is_image(name: &[u8]) -> bool {
    classify(name) == FileClass::Image
}
