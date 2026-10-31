//! C ABI over the lattice fingerprint for native callers, mainly the iOS
//! capture app. The header is `ios/Frameworks/include/fingerprint.h`.
//!
//! The output layout is the capture page's: R, G, then B, `HASH_ROWS`
//! big-endian 32-byte field elements each, the values `Fingerprint::of`
//! writes in hex. Every function returns a status instead of panicking,
//! because unwinding into C or Swift is undefined behaviour.

use std::ffi::{c_char, CStr};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::slice;

use fingerprint::{ELEMENT_BYTES, HASH_ROWS, ORIGINAL};

pub const FINGERPRINT_OK: i32 = 0;
pub const FINGERPRINT_ERROR_NULL: i32 = 1;
pub const FINGERPRINT_ERROR_LENGTH: i32 = 2;
pub const FINGERPRINT_ERROR_ROWS: i32 = 3;
pub const FINGERPRINT_ERROR_INTERNAL: i32 = 4;

/// Bytes `fingerprint_rgb` writes.
pub const FINGERPRINT_BYTES: usize = 3 * HASH_ROWS * ELEMENT_BYTES;

static ALGORITHM: &CStr = c"hyperveritas-ajtai-chacha8-v1";

fn guarded(body: impl FnOnce()) -> i32 {
    catch_unwind(AssertUnwindSafe(body)).map_or(FINGERPRINT_ERROR_INTERNAL, |()| FINGERPRINT_OK)
}

/// The fingerprint algorithm identifier, a static NUL-terminated string.
#[no_mangle]
pub extern "C" fn fingerprint_algorithm() -> *const c_char {
    ALGORITHM.as_ptr()
}

/// Writes the fingerprint of a 1024x512 image, given as three planes of
/// `len` bytes in row-major order, to `out`. Uses every core and blocks
/// until done.
///
/// # Safety
/// `r`, `g` and `b` must point to `len` readable bytes each and `out` to
/// `FINGERPRINT_BYTES` writable bytes that do not overlap them.
#[no_mangle]
pub unsafe extern "C" fn fingerprint_rgb(
    r: *const u8,
    g: *const u8,
    b: *const u8,
    len: usize,
    out: *mut u8,
) -> i32 {
    if r.is_null() || g.is_null() || b.is_null() || out.is_null() {
        return FINGERPRINT_ERROR_NULL;
    }
    if len != ORIGINAL.pixels() {
        return FINGERPRINT_ERROR_LENGTH;
    }
    let channels = [r, g, b].map(|plane| slice::from_raw_parts(plane, len));
    let out = slice::from_raw_parts_mut(out, FINGERPRINT_BYTES);
    guarded(|| fingerprint::hash_image_into(channels, out))
}

/// Writes rows `row_start..row_end` of one channel's fingerprint to `out`,
/// for callers that schedule the work themselves. Uses every core.
///
/// # Safety
/// `pixels` must point to `len` readable bytes and `out` to
/// `(row_end - row_start) * ELEMENT_BYTES` writable bytes that do not overlap them.
#[no_mangle]
pub unsafe extern "C" fn fingerprint_hash_rows(
    pixels: *const u8,
    len: usize,
    row_start: usize,
    row_end: usize,
    out: *mut u8,
) -> i32 {
    if pixels.is_null() || out.is_null() {
        return FINGERPRINT_ERROR_NULL;
    }
    if len != ORIGINAL.pixels() {
        return FINGERPRINT_ERROR_LENGTH;
    }
    if row_start > row_end || row_end > HASH_ROWS {
        return FINGERPRINT_ERROR_ROWS;
    }
    let pixels = slice::from_raw_parts(pixels, len);
    let out = slice::from_raw_parts_mut(out, (row_end - row_start) * ELEMENT_BYTES);
    guarded(|| fingerprint::hash_rows_into(pixels, row_start..row_end, out))
}

/// Fills three planes of `len` bytes with the deterministic test image, whose
/// fingerprint digest is pinned, so the app can check its build at runtime.
///
/// # Safety
/// `r`, `g` and `b` must point to `len` writable bytes each, and no two of
/// them may overlap.
#[no_mangle]
pub unsafe extern "C" fn fingerprint_test_image(
    r: *mut u8,
    g: *mut u8,
    b: *mut u8,
    len: usize,
) -> i32 {
    if r.is_null() || g.is_null() || b.is_null() {
        return FINGERPRINT_ERROR_NULL;
    }
    if len != ORIGINAL.pixels() {
        return FINGERPRINT_ERROR_LENGTH;
    }
    let planes = [r, g, b].map(|plane| slice::from_raw_parts_mut(plane, len));
    guarded(|| {
        let image = fingerprint::test_image();
        for (plane, channel) in planes.into_iter().zip(image.channels()) {
            plane.copy_from_slice(channel);
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn algorithm_matches_the_crate() {
        let name = unsafe { CStr::from_ptr(fingerprint_algorithm()) };
        assert_eq!(name.to_str().unwrap(), fingerprint::ALGORITHM);
    }
}
