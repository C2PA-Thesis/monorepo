//! Plain C ABI for the browser build, small enough to call from hand-written
//! JavaScript without a bindings generator. Pointers are offsets into the
//! module's linear memory; the caller allocates with `alloc`.

use crate::{hash_rows_into, image, MAX_PIXELS};

#[no_mangle]
pub extern "C" fn alloc(len: usize) -> *mut u8 {
    let mut buffer = Vec::<u8>::with_capacity(len);
    let ptr = buffer.as_mut_ptr();
    std::mem::forget(buffer);
    ptr
}

/// Writes rows `row_start..row_end` of one channel's hash to `out`. The page
/// checks the size against `MAX_PIXELS` before it gets here; a longer channel
/// would hash, but no proof could use it.
///
/// # Safety
/// `pixels` must point to `len` readable bytes and `out` to
/// `(row_end - row_start) * ELEMENT_BYTES` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn hash_rows(
    pixels: *const u8,
    len: usize,
    row_start: u32,
    row_end: u32,
    out: *mut u8,
) {
    let pixels = std::slice::from_raw_parts(pixels, len);
    let rows = row_start as usize..row_end as usize;
    let out = std::slice::from_raw_parts_mut(out, rows.len() * crate::ELEMENT_BYTES);
    hash_rows_into(pixels, rows, out);
}

/// The most pixels an original may have, so the page does not repeat it.
#[no_mangle]
pub extern "C" fn max_pixels() -> usize {
    MAX_PIXELS
}

/// Fills `out` with `len` values of one channel of the test image, for the
/// page's self-test.
///
/// # Safety
/// `out` must point to `len` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn fill_test_channel(channel: u32, len: usize, out: *mut u8) {
    let out = std::slice::from_raw_parts_mut(out, len);
    out.copy_from_slice(&image::test_channel(channel as usize, len));
}
