//! The pure part of ABI v2 (`osj.*`): error codes, flags, caps and the
//! validation of guest pointers. The kernel's host functions are thin wrappers
//! over these, so what a hostile guest can do is decided (and tested) here.
//!
//! Convention: every host function returns an `i32` (or `i64`); `>= 0` is
//! success, `< 0` is one of the `ERR_*` codes. A pointer/length that does not lie
//! inside the guest's linear memory is not an error code but a [`Fault`]: the
//! host turns it into a trap and the app dies ("ponteiro invalido"). A length
//! above a function's cap is `ERR_INVAL`.

use core::ops::Range;

pub const ERR_PERM: i32 = -1;
pub const ERR_NOENT: i32 = -2;
pub const ERR_BADF: i32 = -3;
pub const ERR_INVAL: i32 = -4;
pub const ERR_EXIST: i32 = -5;
pub const ERR_NOSPC: i32 = -6;
pub const ERR_MFILE: i32 = -7;
pub const ERR_NOTDIR: i32 = -8;
pub const ERR_ISDIR: i32 = -9;
pub const ERR_NOTEMPTY: i32 = -10;
pub const ERR_NET: i32 = -11;
pub const ERR_NOSYS: i32 = -12;

/// `fs_open` flags.
pub const O_READ: u32 = 1;
pub const O_WRITE: u32 = 2;
pub const O_CREATE: u32 = 4;
pub const O_TRUNC: u32 = 8;
pub const O_APPEND: u32 = 16;
pub const O_ALL: u32 = O_READ | O_WRITE | O_CREATE | O_TRUNC | O_APPEND;

/// `fs_seek` whence.
pub const SEEK_SET: i32 = 0;
pub const SEEK_CUR: i32 = 1;
pub const SEEK_END: i32 = 2;

// ---- per-call caps ----
/// Bytes moved by one `fs_read` / `fs_write`.
pub const MAX_IO: usize = 64 * 1024;
/// Longest path a guest may pass.
pub const MAX_PATH_ARG: usize = 256;
/// Longest `log` message.
pub const MAX_LOG: usize = 512;
/// Longest `draw_text` string.
pub const MAX_TEXT: usize = 4096;
/// Longest window title.
pub const MAX_TITLE: usize = 48;
/// Longest URL.
pub const MAX_URL: usize = 512;
/// Largest `blit_rgba` image, in pixels.
pub const MAX_BLIT_PIXELS: u64 = 1 << 20;
/// Largest PNG (bytes) `draw_image_png` accepts, and its pixel limit.
pub const MAX_PNG_BYTES: usize = 128 * 1024;
pub const MAX_PNG_DIM: u32 = 512;
/// Total `log` bytes a run may print before further output is dropped.
pub const LOG_BUDGET: usize = 16 * 1024;

/// A guest pointer/length outside its linear memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fault;

/// Validates `[ptr, ptr + len)` against a linear memory of `mem_len` bytes.
/// `ptr` and `len` are the raw wasm `i32`s, reinterpreted as `u32` (a negative
/// value is a huge one and is rejected). A zero-length range is valid at any
/// `ptr <= mem_len`. The arithmetic cannot overflow.
pub fn check_range(mem_len: usize, ptr: i32, len: i32) -> Result<Range<usize>, Fault> {
    let (p, l) = (ptr as u32 as u64, len as u32 as u64);
    let end = p + l;
    if end > mem_len as u64 {
        return Err(Fault);
    }
    Ok(p as usize..end as usize)
}

/// Like [`check_range`] but with a per-function cap on `len`; `Ok(Err(code))`
/// when only the cap is exceeded (so the caller returns the error code), and
/// `Err(Fault)` when the range is outside memory. A negative `len` is a fault
/// as well (never a cap violation).
pub fn check_capped(
    mem_len: usize,
    ptr: i32,
    len: i32,
    cap: usize,
) -> Result<Result<Range<usize>, i32>, Fault> {
    if len < 0 {
        return Err(Fault);
    }
    if len as usize > cap {
        // Still validate the pointer so a wild one is a fault, not ERR_INVAL.
        check_range(mem_len, ptr, 0)?;
        return Ok(Err(ERR_INVAL));
    }
    check_range(mem_len, ptr, len).map(Ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_inside() {
        assert_eq!(check_range(100, 0, 100), Ok(0..100));
        assert_eq!(check_range(100, 10, 20), Ok(10..30));
        assert_eq!(check_range(100, 100, 0), Ok(100..100));
        assert_eq!(check_range(0, 0, 0), Ok(0..0));
    }

    #[test]
    fn range_outside() {
        assert_eq!(check_range(100, 0, 101), Err(Fault));
        assert_eq!(check_range(100, 101, 0), Err(Fault));
        assert_eq!(check_range(100, 99, 2), Err(Fault));
        assert_eq!(check_range(0, 0, 1), Err(Fault));
    }

    #[test]
    fn negative_values_are_huge_not_small() {
        assert_eq!(check_range(1 << 20, -1, 1), Err(Fault));
        assert_eq!(check_range(1 << 20, 0, -1), Err(Fault));
        assert!(check_range(usize::MAX, i32::MIN, 0).is_ok());
        assert_eq!(check_range(100, i32::MIN, 1), Err(Fault));
    }

    #[test]
    fn no_overflow_at_the_extremes() {
        assert_eq!(
            check_range(usize::MAX, -1, -1),
            Ok(u32::MAX as usize..(u32::MAX as u64 * 2) as usize)
        );
        assert_eq!(check_range(10, i32::MAX, i32::MAX), Err(Fault));
    }

    #[test]
    fn capped_variants() {
        assert_eq!(check_capped(100, 0, 10, 16), Ok(Ok(0..10)));
        assert_eq!(check_capped(100, 0, 17, 16), Ok(Err(ERR_INVAL)));
        assert_eq!(check_capped(100, 200, 17, 16), Err(Fault));
        assert_eq!(check_capped(100, 0, -5, 16), Err(Fault));
        assert_eq!(check_capped(100, 95, 10, 16), Err(Fault));
    }

    #[test]
    fn error_codes_are_distinct_and_negative() {
        let all = [
            ERR_PERM,
            ERR_NOENT,
            ERR_BADF,
            ERR_INVAL,
            ERR_EXIST,
            ERR_NOSPC,
            ERR_MFILE,
            ERR_NOTDIR,
            ERR_ISDIR,
            ERR_NOTEMPTY,
            ERR_NET,
            ERR_NOSYS,
        ];
        for (i, a) in all.iter().enumerate() {
            assert!(*a < 0);
            for b in &all[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn open_flags_are_disjoint_bits() {
        assert_eq!(O_READ | O_WRITE | O_CREATE | O_TRUNC | O_APPEND, O_ALL);
        assert_eq!(O_ALL, 31);
    }
}
