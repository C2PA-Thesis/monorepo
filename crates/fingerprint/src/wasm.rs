//! Plain C ABI for the browser build, small enough to call from hand-written
//! JavaScript without a bindings generator. Pointers are offsets into the
//! module's linear memory; the caller allocates with `alloc`.

use crate::{hash_rows_into, image, ORIGINAL};

#[no_mangle]
pub extern "C" fn alloc(len: usize) -> *mut u8 {
    let mut buffer = Vec::<u8>::with_capacity(len);
    let ptr = buffer.as_mut_ptr();
    std::mem::forget(buffer);
    ptr
}

/// Writes rows `row_start..row_end` of one channel's hash to `out`.
///
/// # Safety
/// `pixels` must point to `ORIGINAL.pixels()` readable bytes and `out` to
/// `(row_end - row_start) * ELEMENT_BYTES` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn hash_rows(pixels: *const u8, row_start: u32, row_end: u32, out: *mut u8) {
    let pixels = std::slice::from_raw_parts(pixels, ORIGINAL.pixels());
    let rows = row_start as usize..row_end as usize;
    let out = std::slice::from_raw_parts_mut(out, rows.len() * crate::ELEMENT_BYTES);
    hash_rows_into(pixels, rows, out);
}

/// Fills `out` with one channel of the test image, for the page's self-test.
///
/// # Safety
/// `out` must point to `ORIGINAL.pixels()` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn fill_test_channel(channel: u32, out: *mut u8) {
    let out = std::slice::from_raw_parts_mut(out, ORIGINAL.pixels());
    out.copy_from_slice(&image::test_image().channels()[channel as usize]);
}
