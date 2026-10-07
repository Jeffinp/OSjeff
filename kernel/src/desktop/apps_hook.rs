//! Extension points that other fronts connect later.
//!
//! The file manager and the image viewer call these instead of knowing about the
//! WASM apps platform or the settings app. Each returns the message the caller
//! shows to the user (a status line), or `None` when it handled the request.

/// Open a `.wasm` module as an app. The WASM apps platform replaces this body;
/// until then the request is refused with a visible message.
pub(crate) fn open_wasm(_path: &[u8]) -> Option<&'static str> {
    Some("Plataforma de apps indisponivel")
}

/// Use the image at `path` as the desktop wallpaper. The settings front replaces
/// this body; for now it only tells the user.
pub(crate) fn set_wallpaper(_path: &[u8]) -> Option<&'static str> {
    Some("Papel de parede: sera ligado pelas Configuracoes")
}
