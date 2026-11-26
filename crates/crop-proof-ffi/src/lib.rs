//! C ABI over the crop proof for native callers, mainly the iOS capture app.
//! The header is `ios/Frameworks/include/crop_proof.h`.
//!
//! An image is three planes, R, G and B, of `width * height` bytes each in
//! row-major order, the layout `fingerprint-ffi` takes. Every function
//! returns a status instead of panicking, because unwinding into C or Swift
//! is undefined behaviour. The status names the kind of failure;
//! `crop_proof_last_error` gives the reason in words.

use std::{
    any::Any,
    cell::RefCell,
    ffi::{c_char, CStr, CString},
    fmt::Display,
    panic::{catch_unwind, AssertUnwindSafe},
    path::Path,
    ptr, slice,
};

// Every Rust allocation of the library goes through this, so the tables the
// prover frees leave the app's footprint at once. See crop_proof::alloc.
#[global_allocator]
static ALLOCATOR: crop_proof::alloc::DirectLarge = crop_proof::alloc::DirectLarge;

use crop_proof::{Fingerprint, ProverParams, Rect, RgbImage, Size, Turn, VerifierParams};

pub const CROP_PROOF_OK: i32 = 0;
pub const CROP_PROOF_ERROR_NULL: i32 = 1;
/// An image size, rectangle, turn or path the proof does not take.
pub const CROP_PROOF_ERROR_ARGUMENT: i32 = 2;
/// The parameter directory did not load.
pub const CROP_PROOF_ERROR_PARAMETERS: i32 = 3;
/// The proof is longer than the caller's buffer.
pub const CROP_PROOF_ERROR_BUFFER: i32 = 4;
/// The prover failed.
pub const CROP_PROOF_ERROR_PROVE: i32 = 5;
/// The proof does not verify.
pub const CROP_PROOF_ERROR_REJECTED: i32 = 6;
pub const CROP_PROOF_ERROR_INTERNAL: i32 = 7;

/// A buffer of this many bytes holds a proof of the current protocol, which
/// is about 37 KB. A longer proof is reported, never cut.
pub const CROP_PROOF_CAPACITY: usize = 65536;

/// Prover parameters, behind an opaque pointer.
pub struct CropProofProver(ProverParams);

/// Verifier parameters, behind an opaque pointer.
pub struct CropProofVerifier(VerifierParams);

/// A rectangle of the original, in pixels from its top-left corner.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct CropProofRect {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

/// Why a call failed: the status it returns and the reason it keeps.
struct Failure {
    status: i32,
    reason: String,
}

impl Failure {
    fn new(status: i32, reason: impl Display) -> Self {
        Self {
            status,
            reason: reason.to_string(),
        }
    }

    fn null() -> Self {
        Self::new(CROP_PROOF_ERROR_NULL, "a required pointer is null")
    }
}

/// Turns an error of the crop proof into a failure with `status`.
fn failed(status: i32) -> impl Fn(anyhow::Error) -> Failure {
    move |error| Failure::new(status, format!("{error:#}"))
}

thread_local! {
    /// The reason of the last failed call on this thread.
    static LAST_ERROR: RefCell<CString> = RefCell::new(CString::default());
}

/// What a caught panic said.
fn panic_message(panic: &(dyn Any + Send)) -> String {
    panic
        .downcast_ref::<&str>()
        .map(|text| (*text).to_string())
        .or_else(|| panic.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "no message".to_string())
}

/// Runs `body` and gives its status. A panic becomes a status too.
fn guarded(body: impl FnOnce() -> Result<(), Failure>) -> i32 {
    let outcome = catch_unwind(AssertUnwindSafe(body)).unwrap_or_else(|panic| {
        Err(Failure::new(
            CROP_PROOF_ERROR_INTERNAL,
            format!("panicked: {}", panic_message(panic.as_ref())),
        ))
    });
    match outcome {
        Ok(()) => CROP_PROOF_OK,
        Err(failure) => {
            // A C string ends at its first NUL, so none may be inside.
            let reason = CString::new(failure.reason.replace('\0', " ")).unwrap_or_default();
            // `with` panics on a thread that is ending, and this runs outside
            // `catch_unwind`. Such a thread keeps its status and loses the reason.
            let _ = LAST_ERROR.try_with(|last| *last.borrow_mut() = reason);
            failure.status
        }
    }
}

/// The reason of the last failed call on this thread, a NUL-terminated
/// string. It is empty if no call failed. The pointer is valid until the next
/// call into this library on the same thread.
#[no_mangle]
pub extern "C" fn crop_proof_last_error() -> *const c_char {
    LAST_ERROR
        .try_with(|last| last.borrow().as_ptr())
        .unwrap_or(c"".as_ptr())
}

/// Loads parameters from `dir` into a new box behind `out`. `out` is set to
/// null first, so it never holds a stale pointer after a failure.
unsafe fn load<T>(
    dir: *const c_char,
    out: *mut *mut T,
    read: impl FnOnce(&Path) -> anyhow::Result<T>,
) -> i32 {
    guarded(|| {
        if out.is_null() {
            return Err(Failure::null());
        }
        out.write(ptr::null_mut());
        if dir.is_null() {
            return Err(Failure::null());
        }
        let dir = CStr::from_ptr(dir).to_str().map_err(|_| {
            Failure::new(
                CROP_PROOF_ERROR_ARGUMENT,
                "the parameter directory is not UTF-8",
            )
        })?;
        let loaded = read(Path::new(dir)).map_err(failed(CROP_PROOF_ERROR_PARAMETERS))?;
        out.write(Box::into_raw(Box::new(loaded)));
        Ok(())
    })
}

unsafe fn release<T>(handle: *mut T) {
    if !handle.is_null() {
        // A panic in a destructor must not unwind into the caller either.
        let _ = catch_unwind(AssertUnwindSafe(|| drop(Box::from_raw(handle))));
    }
}

/// Loads the prover parameters from the directory `dir`, which holds
/// `metadata.json` and `prover.bin`, and writes a handle to `out`. The
/// parameters stay in memory until `crop_proof_prover_free`.
///
/// # Safety
/// `dir` must point to a NUL-terminated string and `out` to a writable pointer.
#[no_mangle]
pub unsafe extern "C" fn crop_proof_prover_load(
    dir: *const c_char,
    out: *mut *mut CropProofProver,
) -> i32 {
    load(dir, out, |dir| ProverParams::load(dir).map(CropProofProver))
}

/// Frees a prover. Null is allowed.
///
/// # Safety
/// `prover` must be null or a handle from `crop_proof_prover_load` that no
/// call uses and that was not freed before.
#[no_mangle]
pub unsafe extern "C" fn crop_proof_prover_free(prover: *mut CropProofProver) {
    release(prover);
}

/// Loads the verifier parameters from the directory `dir`, which holds
/// `metadata.json` and `verifier.bin`, and writes a handle to `out`.
///
/// # Safety
/// `dir` must point to a NUL-terminated string and `out` to a writable pointer.
#[no_mangle]
pub unsafe extern "C" fn crop_proof_verifier_load(
    dir: *const c_char,
    out: *mut *mut CropProofVerifier,
) -> i32 {
    load(dir, out, |dir| {
        VerifierParams::load(dir).map(CropProofVerifier)
    })
}

/// Frees a verifier. Null is allowed.
///
/// # Safety
/// `verifier` must be null or a handle from `crop_proof_verifier_load` that
/// no call uses and that was not freed before.
#[no_mangle]
pub unsafe extern "C" fn crop_proof_verifier_free(verifier: *mut CropProofVerifier) {
    release(verifier);
}

/// What one proof is about, built from the caller's planes.
struct Statement {
    original: RgbImage,
    published: RgbImage,
    rect: Rect,
    turn: Turn,
    fingerprint: Fingerprint,
}

/// Copies the caller's original and derives the published image and the
/// fingerprint from it. The cheap checks come first, so a bad argument costs
/// no fingerprint.
unsafe fn statement(
    r: *const u8,
    g: *const u8,
    b: *const u8,
    width: usize,
    height: usize,
    rect: CropProofRect,
    turn: u8,
) -> Result<Statement, Failure> {
    if r.is_null() || g.is_null() || b.is_null() {
        return Err(Failure::null());
    }
    let argument = failed(CROP_PROOF_ERROR_ARGUMENT);
    let size = Size { width, height };
    // Checked before the planes are read: the size says how long they are.
    size.check().map_err(&argument)?;
    let channels = [r, g, b].map(|plane| slice::from_raw_parts(plane, size.pixels()).to_vec());
    let original = RgbImage::new(size, channels).map_err(&argument)?;
    let rect = Rect {
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: rect.height,
    };
    let turn = Turn::new(turn).map_err(&argument)?;
    let published = crop_proof::edit(&original, rect, turn).map_err(&argument)?;
    let fingerprint = Fingerprint::of(&original).map_err(&argument)?;
    Ok(Statement {
        original,
        published,
        rect,
        turn,
        fingerprint,
    })
}

/// Proves that the `rect` of the original, turned `turn` quarter turns
/// clockwise, is a crop of the image with the original's fingerprint. The
/// published image and the fingerprint are computed here from the planes.
///
/// Writes the proof to `out` and its length to `length`. If the proof is
/// longer than `capacity`, nothing is written to `out`, `length` gets the
/// length needed and the status is `CROP_PROOF_ERROR_BUFFER`. Uses every
/// core and blocks until done.
///
/// # Safety
/// `prover` must be a live handle from `crop_proof_prover_load`. `r`, `g`
/// and `b` must point to `width * height` readable bytes each, `out` to
/// `capacity` writable bytes and `length` to a writable `usize`.
#[no_mangle]
pub unsafe extern "C" fn crop_proof_prove(
    prover: *const CropProofProver,
    r: *const u8,
    g: *const u8,
    b: *const u8,
    width: usize,
    height: usize,
    rect: CropProofRect,
    turn: u8,
    out: *mut u8,
    capacity: usize,
    length: *mut usize,
) -> i32 {
    guarded(|| {
        if prover.is_null() || out.is_null() || length.is_null() {
            return Err(Failure::null());
        }
        length.write(0);
        let Statement {
            original,
            published,
            rect,
            turn,
            fingerprint,
        } = statement(r, g, b, width, height, rect, turn)?;
        let proof = crop_proof::prove(
            &(*prover).0,
            &original,
            &published,
            rect,
            turn,
            &fingerprint,
        )
        .map_err(failed(CROP_PROOF_ERROR_PROVE))?;
        deliver(&proof, out, capacity, length)
    })
}

/// Hands `proof` to the caller: its length always, its bytes only if the
/// buffer of `capacity` bytes holds them all.
unsafe fn deliver(
    proof: &[u8],
    out: *mut u8,
    capacity: usize,
    length: *mut usize,
) -> Result<(), Failure> {
    length.write(proof.len());
    if proof.len() > capacity {
        return Err(Failure::new(
            CROP_PROOF_ERROR_BUFFER,
            format!(
                "the proof is {} bytes and the buffer holds {capacity}",
                proof.len()
            ),
        ));
    }
    ptr::copy_nonoverlapping(proof.as_ptr(), out, proof.len());
    Ok(())
}

/// Checks `proof` as a reader would, against the published image and the
/// fingerprint that the same original gives. It is the check to run on the
/// device right after proving. The status is `CROP_PROOF_OK` if the proof
/// verifies and `CROP_PROOF_ERROR_REJECTED` if it does not.
///
/// # Safety
/// `verifier` must be a live handle from `crop_proof_verifier_load`. `r`,
/// `g` and `b` must point to `width * height` readable bytes each and
/// `proof` to `proof_length` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn crop_proof_verify(
    verifier: *const CropProofVerifier,
    r: *const u8,
    g: *const u8,
    b: *const u8,
    width: usize,
    height: usize,
    rect: CropProofRect,
    turn: u8,
    proof: *const u8,
    proof_length: usize,
) -> i32 {
    guarded(|| {
        if verifier.is_null() || proof.is_null() {
            return Err(Failure::null());
        }
        let Statement {
            published,
            rect,
            turn,
            fingerprint,
            ..
        } = statement(r, g, b, width, height, rect, turn)?;
        let proof = slice::from_raw_parts(proof, proof_length);
        crop_proof::verify(&(*verifier).0, &published, rect, turn, &fingerprint, proof)
            .map_err(failed(CROP_PROOF_ERROR_REJECTED))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn last_error() -> String {
        // SAFETY: the pointer is a NUL-terminated string of this thread.
        let reason = unsafe { CStr::from_ptr(crop_proof_last_error()) };
        reason.to_string_lossy().into_owned()
    }

    #[test]
    fn a_panic_becomes_a_status_and_a_reason() {
        assert_eq!(guarded(|| panic!("x")), CROP_PROOF_ERROR_INTERNAL);
        assert_eq!(last_error(), "panicked: x");

        let formatted = || -> Result<(), Failure> { panic!("round {}", 3) };
        assert_eq!(guarded(formatted), CROP_PROOF_ERROR_INTERNAL);
        assert_eq!(last_error(), "panicked: round 3");

        let odd = || -> Result<(), Failure> { std::panic::panic_any(7) };
        assert_eq!(guarded(odd), CROP_PROOF_ERROR_INTERNAL);
        assert_eq!(last_error(), "panicked: no message");
    }

    #[test]
    fn a_success_keeps_the_reason_of_the_failure_before_it() {
        let refused = || Err(Failure::new(CROP_PROOF_ERROR_ARGUMENT, "a\0b"));
        assert_eq!(guarded(refused), CROP_PROOF_ERROR_ARGUMENT);
        assert_eq!(last_error(), "a b", "a NUL inside would cut the reason");
        assert_eq!(guarded(|| Ok(())), CROP_PROOF_OK);
        assert_eq!(last_error(), "a b");
    }

    #[test]
    fn a_thread_without_failures_has_an_empty_reason() {
        let reason = std::thread::spawn(last_error).join().unwrap();
        assert_eq!(reason, "");
    }

    #[test]
    fn a_short_buffer_gets_the_length_and_no_bytes() {
        let proof = [7u8; 24];
        let mut out = [0xa5u8; 32];
        let mut length = 0;
        let short = || unsafe { deliver(&proof, out.as_mut_ptr(), 23, &mut length) };
        assert_eq!(guarded(short), CROP_PROOF_ERROR_BUFFER);
        assert_eq!(length, 24);
        assert_eq!(out, [0xa5; 32], "nothing was written");
        assert_eq!(
            last_error(),
            "the proof is 24 bytes and the buffer holds 23"
        );
    }

    #[test]
    fn a_buffer_that_fits_gets_the_proof() {
        let proof = [7u8; 24];
        let mut out = [0xa5u8; 32];
        let mut length = 0;
        let exact = || unsafe { deliver(&proof, out.as_mut_ptr(), 24, &mut length) };
        assert_eq!(guarded(exact), CROP_PROOF_OK);
        assert_eq!(length, 24);
        assert_eq!(out[..24], proof);
        assert_eq!(out[24..], [0xa5; 8], "nothing past the proof was written");
    }
}
