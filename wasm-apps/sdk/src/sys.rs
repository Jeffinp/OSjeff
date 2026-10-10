//! Raw `osj.*` imports (ABI v2). `osj` is a historical abbreviation kept as the import
//! module name for compatibility. Prefer the safe wrappers in the crate root.
//! On non-wasm targets the functions are stubs so the crate type-checks anywhere.

#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "osj")]
unsafe extern "C" {
    pub fn set_title(ptr: *const u8, len: i32) -> i32;
    pub fn get_size() -> i64;
    pub fn request_redraw();
    pub fn fill_rect(x: i32, y: i32, w: i32, h: i32, rgb: i32);
    pub fn draw_text(x: i32, y: i32, ptr: *const u8, len: i32, rgb: i32, scale: i32) -> i32;
    pub fn lang() -> i32;
    pub fn blit_rgba(ptr: *const u8, w: i32, h: i32, dx: i32, dy: i32) -> i32;
    pub fn draw_image_png(ptr: *const u8, len: i32, x: i32, y: i32) -> i32;
    pub fn now_ms() -> i64;
    pub fn monotonic_ms() -> i64;
    pub fn random() -> i32;
    pub fn log(ptr: *const u8, len: i32) -> i32;
    pub fn exit(code: i32);
    pub fn clip_get(ptr: *mut u8, cap: i32) -> i32;
    pub fn clip_set(ptr: *const u8, len: i32) -> i32;
    pub fn fs_open(ptr: *const u8, len: i32, flags: i32) -> i32;
    pub fn fs_close(fd: i32) -> i32;
    pub fn fs_read(fd: i32, ptr: *mut u8, len: i32) -> i32;
    pub fn fs_write(fd: i32, ptr: *const u8, len: i32) -> i32;
    pub fn fs_seek(fd: i32, off: i64, whence: i32) -> i64;
    pub fn fs_stat(ptr: *const u8, len: i32, out: *mut u8) -> i32;
    pub fn fs_readdir(ptr: *const u8, len: i32, index: i32, out: *mut u8, cap: i32) -> i32;
    pub fn fs_mkdir(ptr: *const u8, len: i32) -> i32;
    pub fn fs_unlink(ptr: *const u8, len: i32) -> i32;
    pub fn fs_rename(a: *const u8, an: i32, b: *const u8, bn: i32) -> i32;
    pub fn net_http_get(url: *const u8, ulen: i32, out: *mut u8, cap: i32) -> i32;
}

#[cfg(not(target_arch = "wasm32"))]
mod stubs {
    #![allow(clippy::missing_safety_doc, unused_variables)]
    pub unsafe fn set_title(ptr: *const u8, len: i32) -> i32 {
        0
    }
    pub unsafe fn get_size() -> i64 {
        0
    }
    pub unsafe fn request_redraw() {}
    pub unsafe fn fill_rect(x: i32, y: i32, w: i32, h: i32, rgb: i32) {}
    pub unsafe fn draw_text(x: i32, y: i32, ptr: *const u8, len: i32, rgb: i32, scale: i32) -> i32 {
        0
    }
    pub unsafe fn lang() -> i32 {
        0
    }
    pub unsafe fn blit_rgba(ptr: *const u8, w: i32, h: i32, dx: i32, dy: i32) -> i32 {
        0
    }
    pub unsafe fn draw_image_png(ptr: *const u8, len: i32, x: i32, y: i32) -> i32 {
        0
    }
    pub unsafe fn now_ms() -> i64 {
        0
    }
    pub unsafe fn monotonic_ms() -> i64 {
        0
    }
    pub unsafe fn random() -> i32 {
        0
    }
    pub unsafe fn log(ptr: *const u8, len: i32) -> i32 {
        0
    }
    pub unsafe fn exit(code: i32) {}
    pub unsafe fn clip_get(ptr: *mut u8, cap: i32) -> i32 {
        0
    }
    pub unsafe fn clip_set(ptr: *const u8, len: i32) -> i32 {
        0
    }
    pub unsafe fn fs_open(ptr: *const u8, len: i32, flags: i32) -> i32 {
        -12
    }
    pub unsafe fn fs_close(fd: i32) -> i32 {
        0
    }
    pub unsafe fn fs_read(fd: i32, ptr: *mut u8, len: i32) -> i32 {
        -12
    }
    pub unsafe fn fs_write(fd: i32, ptr: *const u8, len: i32) -> i32 {
        -12
    }
    pub unsafe fn fs_seek(fd: i32, off: i64, whence: i32) -> i64 {
        -12
    }
    pub unsafe fn fs_stat(ptr: *const u8, len: i32, out: *mut u8) -> i32 {
        -12
    }
    pub unsafe fn fs_readdir(ptr: *const u8, len: i32, index: i32, out: *mut u8, cap: i32) -> i32 {
        -12
    }
    pub unsafe fn fs_mkdir(ptr: *const u8, len: i32) -> i32 {
        -12
    }
    pub unsafe fn fs_unlink(ptr: *const u8, len: i32) -> i32 {
        -12
    }
    pub unsafe fn fs_rename(a: *const u8, an: i32, b: *const u8, bn: i32) -> i32 {
        -12
    }
    pub unsafe fn net_http_get(url: *const u8, ulen: i32, out: *mut u8, cap: i32) -> i32 {
        -12
    }
}
#[cfg(not(target_arch = "wasm32"))]
pub use stubs::*;
