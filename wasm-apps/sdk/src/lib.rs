//! OSjeff app SDK (ABI v2).
//!
//! An app is one `.wasm` file: code + a manifest section (`manifest!`) + an
//! optional icon (`icon!`). Implement [`App`], then `export_app!(MyApp);`.
//!
//! ```ignore
//! #![no_std]
//! use osjeff_sdk::*;
//!
//! manifest!("id=hello\nname=Hello\nversion=1.0.0\nwin_w=360\nwin_h=220\n");
//!
//! #[derive(Default)]
//! struct Hello { n: u32 }
//! impl App for Hello {
//!     fn new() -> Self { Hello::default() }
//!     fn on_key(&mut self, _code: i32, _mods: i32) { self.n += 1; }
//!     fn render(&mut self, c: &mut Canvas) {
//!         c.clear(0x10141F);
//!         c.text(16, 16, "Ola!", 0xFFFFFF, 2);
//!     }
//! }
//! export_app!(Hello);
//! ```
//!
//! Every host call can fail with an [`Errno`] (denied permission, quota, ...); a
//! pointer outside the app's memory kills the app, which safe code cannot cause.

#![no_std]

pub mod sys;

use core::fmt;

// ------------------------------------------------------------------ errors

/// A negative ABI error code.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Errno(pub i32);

impl Errno {
    /// Permission denied, or the path tried to leave the sandbox.
    pub const PERM: Errno = Errno(-1);
    pub const NOENT: Errno = Errno(-2);
    pub const BADF: Errno = Errno(-3);
    pub const INVAL: Errno = Errno(-4);
    pub const EXIST: Errno = Errno(-5);
    /// Disk quota exhausted.
    pub const NOSPC: Errno = Errno(-6);
    /// Too many open descriptors.
    pub const MFILE: Errno = Errno(-7);
    pub const NOTDIR: Errno = Errno(-8);
    pub const ISDIR: Errno = Errno(-9);
    pub const NOTEMPTY: Errno = Errno(-10);
    pub const NET: Errno = Errno(-11);
    pub const NOSYS: Errno = Errno(-12);
}

fn check(r: i32) -> Result<i32, Errno> {
    if r < 0 {
        Err(Errno(r))
    } else {
        Ok(r)
    }
}

// ------------------------------------------------------------------ the app trait

/// An OSjeff app. Only `new` and `render` are required.
pub trait App: Sized {
    fn new() -> Self;
    /// Key press: `code` is ASCII, 10 Enter, 27 Esc, 8 Backspace, 9 Tab, 127 Delete,
    /// or one of the `KEY_*` constants; `mods` is a bit set of `MOD_*`.
    fn on_key(&mut self, _code: i32, _mods: i32) {}
    /// A printable character (already translated by the keyboard layout).
    fn on_text(&mut self, _ch: u32) {}
    /// Pointer position (content-local) and buttons (`BTN_*`).
    fn on_pointer(&mut self, _x: i32, _y: i32, _buttons: i32) {}
    fn on_scroll(&mut self, _dx: i32, _dy: i32) {}
    /// The content area changed size (also called once at start).
    fn on_resize(&mut self, _w: i32, _h: i32) {}
    /// Every `tick_ms` of the manifest; `dt_ms` since the previous tick.
    fn on_tick(&mut self, _dt_ms: i32) {}
    /// The window is closing: last chance to save.
    fn on_close(&mut self) {}
    /// Paint. The surface keeps the previous frame; a full repaint is never required.
    fn render(&mut self, c: &mut Canvas);
}

pub const KEY_LEFT: i32 = 0x100;
pub const KEY_RIGHT: i32 = 0x101;
pub const KEY_UP: i32 = 0x102;
pub const KEY_DOWN: i32 = 0x103;
pub const KEY_HOME: i32 = 0x104;
pub const KEY_END: i32 = 0x105;
pub const KEY_PAGE_UP: i32 = 0x106;
pub const KEY_PAGE_DOWN: i32 = 0x107;
pub const MOD_SHIFT: i32 = 1;
pub const MOD_CTRL: i32 = 2;
pub const MOD_ALT: i32 = 4;
pub const BTN_LEFT: i32 = 1;
pub const BTN_RIGHT: i32 = 2;

/// Declares the exports the host calls and wires them to an [`App`] value.
#[macro_export]
macro_rules! export_app {
    ($t:ty) => {
        static mut __OSJ_APP: Option<$t> = None;
        fn __osj_app() -> &'static mut $t {
            // SAFETY: wasm32-unknown-unknown here is single threaded and the host never re-enters
            // an export while one runs, so this is the only live reference.
            unsafe {
                (*core::ptr::addr_of_mut!(__OSJ_APP)).get_or_insert_with(<$t as $crate::App>::new)
            }
        }
        #[no_mangle]
        pub extern "C" fn on_key(code: i32, mods: i32) {
            $crate::App::on_key(__osj_app(), code, mods)
        }
        #[no_mangle]
        pub extern "C" fn on_text(ch: i32) {
            $crate::App::on_text(__osj_app(), ch as u32)
        }
        #[no_mangle]
        pub extern "C" fn on_pointer(x: i32, y: i32, buttons: i32) {
            $crate::App::on_pointer(__osj_app(), x, y, buttons)
        }
        #[no_mangle]
        pub extern "C" fn on_scroll(dx: i32, dy: i32) {
            $crate::App::on_scroll(__osj_app(), dx, dy)
        }
        #[no_mangle]
        pub extern "C" fn on_resize(w: i32, h: i32) {
            $crate::App::on_resize(__osj_app(), w, h)
        }
        #[no_mangle]
        pub extern "C" fn on_tick(dt: i32) {
            $crate::App::on_tick(__osj_app(), dt)
        }
        #[no_mangle]
        pub extern "C" fn on_close() {
            $crate::App::on_close(__osj_app())
        }
        #[no_mangle]
        pub extern "C" fn render() {
            let mut c = $crate::Canvas::new();
            $crate::App::render(__osj_app(), &mut c)
        }
    };
}

/// Embeds the manifest text as the `osjeff.manifest` custom section.
#[macro_export]
macro_rules! manifest {
    ($text:expr) => {
        const _: () = {
            const T: &[u8] = $text.as_bytes();
            #[used]
            #[link_section = "osjeff.manifest"]
            static M: [u8; T.len()] = {
                let mut a = [0u8; T.len()];
                let mut i = 0;
                while i < T.len() {
                    a[i] = T[i];
                    i += 1;
                }
                a
            };
        };
    };
}

/// Embeds a PNG (<= 64x64) as the `osjeff.icon` custom section:
/// `icon!(include_bytes!("icon.png"));`
#[macro_export]
macro_rules! icon {
    ($bytes:expr) => {
        const _: () = {
            const B: &[u8] = $bytes;
            #[used]
            #[link_section = "osjeff.icon"]
            static I: [u8; B.len()] = {
                let mut a = [0u8; B.len()];
                let mut i = 0;
                while i < B.len() {
                    a[i] = B[i];
                    i += 1;
                }
                a
            };
        };
    };
}

/// `log!("x = {}", x)` -> serial log (<= 512 bytes per call, 16 KiB per run).
#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => {{
        let mut b = $crate::StrBuf::<200>::new();
        let _ = core::fmt::Write::write_fmt(&mut b, format_args!($($arg)*));
        $crate::log(b.as_str());
    }};
}

#[cfg(target_arch = "wasm32")]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let mut b = StrBuf::<200>::new();
    let _ = fmt::Write::write_fmt(&mut b, format_args!("panic: {}", info.message()));
    log(b.as_str());
    core::arch::wasm32::unreachable()
}

// ------------------------------------------------------------------ small helpers

/// A fixed-capacity string buffer implementing `fmt::Write` (no allocator needed).
pub struct StrBuf<const N: usize> {
    buf: [u8; N],
    len: usize,
}

impl<const N: usize> StrBuf<N> {
    pub const fn new() -> Self {
        StrBuf {
            buf: [0; N],
            len: 0,
        }
    }
    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }
    pub fn as_bytes(&self) -> &[u8] {
        &self.buf[..self.len]
    }
    pub fn clear(&mut self) {
        self.len = 0;
    }
}

impl<const N: usize> Default for StrBuf<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> fmt::Write for StrBuf<N> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let n = s.len().min(N - self.len);
        self.buf[self.len..self.len + n].copy_from_slice(&s.as_bytes()[..n]);
        self.len += n;
        Ok(())
    }
}

// ------------------------------------------------------------------ window, time, system

/// Window title (<= 48 printable ASCII bytes).
pub fn set_title(title: &str) -> Result<(), Errno> {
    check(unsafe { sys::set_title(title.as_ptr(), title.len() as i32) }).map(|_| ())
}

/// Size of the content area.
pub fn size() -> (i32, i32) {
    let v = unsafe { sys::get_size() };
    ((v >> 32) as i32, v as i32)
}

/// Ask for a `render` even without input.
pub fn request_redraw() {
    unsafe { sys::request_redraw() }
}

/// Wall clock: milliseconds since local midnight.
pub fn now_ms() -> i64 {
    unsafe { sys::now_ms() }
}

/// Milliseconds since boot.
pub fn monotonic_ms() -> i64 {
    unsafe { sys::monotonic_ms() }
}

/// A random number from the kernel's generator (a ChaCha20 DRBG seeded from hardware and timing
/// entropy, `docs/design/entropy.md`). Fit for keys and tokens when the machine has a hardware source
/// or enough timing entropy (the kernel logs `RNG:` lines on the serial port); best effort otherwise.
pub fn random() -> u32 {
    unsafe { sys::random() as u32 }
}

/// Write a line to the serial log.
pub fn log(msg: &str) {
    let m = &msg.as_bytes()[..msg.len().min(500)];
    unsafe {
        sys::log(m.as_ptr(), m.len() as i32);
    }
}

/// Leave the app (clean exit).
pub fn exit(code: i32) -> ! {
    unsafe { sys::exit(code) };
    #[cfg(target_arch = "wasm32")]
    core::arch::wasm32::unreachable();
    #[cfg(not(target_arch = "wasm32"))]
    loop {}
}

// ------------------------------------------------------------------ clipboard

/// Reads the clipboard (needs `clipboard=rw`); returns the byte count.
pub fn clip_get(buf: &mut [u8]) -> Result<usize, Errno> {
    let cap = buf.len().min(256);
    check(unsafe { sys::clip_get(buf.as_mut_ptr(), cap as i32) }).map(|n| n as usize)
}

/// Replaces the clipboard (needs `clipboard=rw`); at most 256 bytes.
pub fn clip_set(data: &[u8]) -> Result<(), Errno> {
    let d = &data[..data.len().min(256)];
    check(unsafe { sys::clip_set(d.as_ptr(), d.len() as i32) }).map(|_| ())
}

// ------------------------------------------------------------------ canvas

/// The window's drawing surface. Colors are `0xRRGGBB`; coordinates are
/// content-local and clipped by the host.
pub struct Canvas(());

impl Canvas {
    #[doc(hidden)]
    pub fn new() -> Canvas {
        Canvas(())
    }

    pub fn size(&self) -> (i32, i32) {
        size()
    }

    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, rgb: u32) {
        unsafe { sys::fill_rect(x, y, w, h, rgb as i32) }
    }

    /// Fills the whole surface.
    pub fn clear(&mut self, rgb: u32) {
        let (w, h) = size();
        self.fill_rect(0, 0, w, h, rgb);
    }

    /// Draws ASCII text; each glyph cell is `6 * scale` x `8 * scale` pixels.
    pub fn text(&mut self, x: i32, y: i32, s: &str, rgb: u32, scale: i32) {
        // The host takes at most 4096 bytes per call.
        let mut off = 0;
        let mut cx = x;
        while off < s.len() {
            let n = (s.len() - off).min(4000);
            unsafe {
                sys::draw_text(cx, y, s.as_ptr().add(off), n as i32, rgb as i32, scale);
            }
            cx += n as i32 * 6 * scale;
            off += n;
        }
    }

    /// Width in pixels of `s` at `scale`.
    pub fn text_width(s: &str, scale: i32) -> i32 {
        s.len() as i32 * 6 * scale
    }

    /// 1:1 copy of an RGBA image (`w * h * 4` bytes; alpha ignored).
    pub fn blit_rgba(&mut self, px: &[u8], w: i32, h: i32, x: i32, y: i32) -> Result<(), Errno> {
        if (w.max(0) as usize) * (h.max(0) as usize) * 4 > px.len() {
            return Err(Errno::INVAL);
        }
        check(unsafe { sys::blit_rgba(px.as_ptr(), w, h, x, y) }).map(|_| ())
    }

    /// Decodes and draws a PNG (<= 512x512, <= 128 KiB); returns its size.
    pub fn png(&mut self, data: &[u8], x: i32, y: i32) -> Result<(i32, i32), Errno> {
        let r = check(unsafe { sys::draw_image_png(data.as_ptr(), data.len() as i32, x, y) })?;
        Ok((r >> 16, r & 0xFFFF))
    }
}

// ------------------------------------------------------------------ files

pub const O_READ: u32 = 1;
pub const O_WRITE: u32 = 2;
pub const O_CREATE: u32 = 4;
pub const O_TRUNC: u32 = 8;
pub const O_APPEND: u32 = 16;
pub const SEEK_SET: i32 = 0;
pub const SEEK_CUR: i32 = 1;
pub const SEEK_END: i32 = 2;

/// An open file in the app's sandbox (closed on drop). `/` is the app's own folder
/// (`fs=own`) or the user's home (`fs=home`); `..` above it fails with `Errno::PERM`.
pub struct File(i32);

impl File {
    pub fn open(path: &str, flags: u32) -> Result<File, Errno> {
        let fd = check(unsafe { sys::fs_open(path.as_ptr(), path.len() as i32, flags as i32) })?;
        Ok(File(fd))
    }

    /// Reads up to `buf.len()` bytes (<= 64 KiB per call); `Ok(0)` at the end.
    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, Errno> {
        let n = buf.len().min(65536);
        check(unsafe { sys::fs_read(self.0, buf.as_mut_ptr(), n as i32) }).map(|n| n as usize)
    }

    /// Writes some of `data` (<= 64 KiB per call); returns the count written.
    pub fn write(&mut self, data: &[u8]) -> Result<usize, Errno> {
        let n = data.len().min(65536);
        check(unsafe { sys::fs_write(self.0, data.as_ptr(), n as i32) }).map(|n| n as usize)
    }

    pub fn write_all(&mut self, mut data: &[u8]) -> Result<(), Errno> {
        while !data.is_empty() {
            let n = self.write(data)?;
            if n == 0 {
                return Err(Errno::NOSPC);
            }
            data = &data[n..];
        }
        Ok(())
    }

    /// Reads until the buffer is full or the file ends; returns the count.
    pub fn read_full(&mut self, buf: &mut [u8]) -> Result<usize, Errno> {
        let mut got = 0;
        while got < buf.len() {
            let n = self.read(&mut buf[got..])?;
            if n == 0 {
                break;
            }
            got += n;
        }
        Ok(got)
    }

    pub fn seek(&mut self, off: i64, whence: i32) -> Result<u64, Errno> {
        let r = unsafe { sys::fs_seek(self.0, off, whence) };
        if r < 0 {
            Err(Errno(r as i32))
        } else {
            Ok(r as u64)
        }
    }
}

impl Drop for File {
    fn drop(&mut self) {
        unsafe {
            sys::fs_close(self.0);
        }
    }
}

/// Kind of a directory entry or stat result.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    File,
    Dir,
}

/// `(kind, size)` of a path.
pub fn stat(path: &str) -> Result<(Kind, u64), Errno> {
    let mut out = [0u8; 16];
    check(unsafe { sys::fs_stat(path.as_ptr(), path.len() as i32, out.as_mut_ptr()) })?;
    let kind = if u32::from_le_bytes([out[0], out[1], out[2], out[3]]) == 2 {
        Kind::Dir
    } else {
        Kind::File
    };
    let mut sz = [0u8; 8];
    sz.copy_from_slice(&out[4..12]);
    Ok((kind, u64::from_le_bytes(sz)))
}

/// The `index`-th entry of folder `path`: writes its name into `name` and returns
/// `(kind, name_len)`; `None` past the last entry.
pub fn read_dir(
    path: &str,
    index: usize,
    name: &mut [u8; 49],
) -> Result<Option<(Kind, usize)>, Errno> {
    let r = check(unsafe {
        sys::fs_readdir(
            path.as_ptr(),
            path.len() as i32,
            index as i32,
            name.as_mut_ptr(),
            name.len() as i32,
        )
    })?;
    if r == 0 {
        return Ok(None);
    }
    let kind = if name[0] == 2 { Kind::Dir } else { Kind::File };
    let n = r as usize;
    name.copy_within(1..1 + n, 0);
    Ok(Some((kind, n)))
}

pub fn mkdir(path: &str) -> Result<(), Errno> {
    check(unsafe { sys::fs_mkdir(path.as_ptr(), path.len() as i32) }).map(|_| ())
}

/// Removes a file or an empty folder.
pub fn unlink(path: &str) -> Result<(), Errno> {
    check(unsafe { sys::fs_unlink(path.as_ptr(), path.len() as i32) }).map(|_| ())
}

pub fn rename(from: &str, to: &str) -> Result<(), Errno> {
    check(unsafe {
        sys::fs_rename(
            from.as_ptr(),
            from.len() as i32,
            to.as_ptr(),
            to.len() as i32,
        )
    })
    .map(|_| ())
}

// ------------------------------------------------------------------ network

/// `GET url` (needs `net=http`; public hosts only, 8 s, <= 256 KiB): returns the
/// number of body bytes written to `out`.
pub fn http_get(url: &str, out: &mut [u8]) -> Result<usize, Errno> {
    let cap = out.len().min(256 * 1024);
    check(unsafe {
        sys::net_http_get(url.as_ptr(), url.len() as i32, out.as_mut_ptr(), cap as i32)
    })
    .map(|n| n as usize)
}
