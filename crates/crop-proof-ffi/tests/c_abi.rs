//! The C ABI through its exported symbols: what it refuses, and that the
//! header says the same as the crate. A whole proof needs the parameters of
//! `provenance setup`, so `c/check.sh` and the app's benchmark make one.

use std::{
    ffi::{CStr, CString},
    fs,
    ptr::{self, NonNull},
};

use crop_proof_ffi::*;

const HEADER: &str = include_str!("../../../ios/Frameworks/include/crop_proof.h");
const WIDTH: usize = 8;
const HEIGHT: usize = 4;
const WHOLE: CropProofRect = CropProofRect {
    x: 0,
    y: 0,
    width: WIDTH,
    height: HEIGHT,
};

fn last_error() -> String {
    let reason = unsafe { CStr::from_ptr(crop_proof_last_error()) };
    reason.to_str().unwrap().to_string()
}

/// `crop_proof_prove` with a handle that must never be read, because every
/// call here fails on its arguments first.
fn prove(
    planes: [*const u8; 3],
    width: usize,
    height: usize,
    rect: CropProofRect,
    turn: u8,
) -> i32 {
    let prover = NonNull::<CropProofProver>::dangling().as_ptr();
    let mut out = [0u8; 16];
    let mut length = usize::MAX;
    let [r, g, b] = planes;
    let status = unsafe {
        crop_proof_prove(
            prover,
            r,
            g,
            b,
            width,
            height,
            rect,
            turn,
            out.as_mut_ptr(),
            out.len(),
            &mut length,
        )
    };
    assert_eq!(length, 0, "no proof was made");
    status
}

/// The same for `crop_proof_verify`.
fn verify(
    planes: [*const u8; 3],
    width: usize,
    height: usize,
    rect: CropProofRect,
    turn: u8,
) -> i32 {
    let verifier = NonNull::<CropProofVerifier>::dangling().as_ptr();
    let proof = [0u8; 16];
    let [r, g, b] = planes;
    unsafe {
        crop_proof_verify(
            verifier,
            r,
            g,
            b,
            width,
            height,
            rect,
            turn,
            proof.as_ptr(),
            proof.len(),
        )
    }
}

#[test]
fn bad_images_and_edits_are_refused_before_any_work() {
    let plane = [0u8; WIDTH * HEIGHT];
    let p = plane.as_ptr();
    for call in [prove, verify] {
        assert_eq!(
            call([p, ptr::null(), p], WIDTH, HEIGHT, WHOLE, 0),
            CROP_PROOF_ERROR_NULL
        );
        // Sizes the proof does not take; the planes are not read.
        for (width, height) in [(0, HEIGHT), (WIDTH, 0), (1025, 512), (usize::MAX, 2)] {
            assert_eq!(
                call([p, p, p], width, height, WHOLE, 0),
                CROP_PROOF_ERROR_ARGUMENT,
                "{width}x{height}"
            );
        }
        for rect in [
            CropProofRect { x: 1, ..WHOLE },
            CropProofRect { y: 1, ..WHOLE },
            CropProofRect { width: 0, ..WHOLE },
            CropProofRect {
                x: usize::MAX,
                width: 2,
                ..WHOLE
            },
        ] {
            assert_eq!(
                call([p, p, p], WIDTH, HEIGHT, rect, 0),
                CROP_PROOF_ERROR_ARGUMENT,
                "{rect:?}"
            );
            assert!(last_error().contains("does not fit"), "{}", last_error());
        }
        assert_eq!(
            call([p, p, p], WIDTH, HEIGHT, WHOLE, 4),
            CROP_PROOF_ERROR_ARGUMENT
        );
        assert!(last_error().contains("quarter turns"), "{}", last_error());
    }
}

#[test]
fn null_handles_and_buffers_are_refused() {
    let plane = [0u8; WIDTH * HEIGHT];
    let p = plane.as_ptr();
    let prover = NonNull::<CropProofProver>::dangling().as_ptr();
    let verifier = NonNull::<CropProofVerifier>::dangling().as_ptr();
    let mut out = [0u8; 16];
    let mut length = 0;
    unsafe {
        for (prover, out, length) in [
            (ptr::null_mut(), out.as_mut_ptr(), &raw mut length),
            (prover, ptr::null_mut(), &raw mut length),
            (prover, out.as_mut_ptr(), ptr::null_mut()),
        ] {
            assert_eq!(
                crop_proof_prove(prover, p, p, p, WIDTH, HEIGHT, WHOLE, 0, out, 16, length),
                CROP_PROOF_ERROR_NULL
            );
        }
        for (verifier, proof) in [(ptr::null_mut(), p), (verifier, ptr::null())] {
            assert_eq!(
                crop_proof_verify(verifier, p, p, p, WIDTH, HEIGHT, WHOLE, 0, proof, 16),
                CROP_PROOF_ERROR_NULL
            );
        }
        crop_proof_prover_free(ptr::null_mut());
        crop_proof_verifier_free(ptr::null_mut());
    }
}

#[test]
fn loading_says_why_it_failed() {
    assert_eq!(last_error(), "", "nothing failed on this thread yet");

    let empty = tempfile::tempdir().unwrap();
    let path = CString::new(empty.path().to_str().unwrap()).unwrap();
    let stale = NonNull::<CropProofProver>::dangling().as_ptr();
    let mut prover = stale;
    let mut verifier = NonNull::<CropProofVerifier>::dangling().as_ptr();
    unsafe {
        assert_eq!(
            crop_proof_prover_load(path.as_ptr(), &mut prover),
            CROP_PROOF_ERROR_PARAMETERS
        );
        assert!(prover.is_null(), "a failed load leaves no handle");
        assert!(last_error().contains("metadata.json"), "{}", last_error());

        // Parameters of another protocol, as an older setup would leave them.
        let metadata = r#"{"protocol": "another", "num_vars": 19, "files": {}}"#;
        fs::write(empty.path().join("metadata.json"), metadata).unwrap();
        assert_eq!(
            crop_proof_verifier_load(path.as_ptr(), &mut verifier),
            CROP_PROOF_ERROR_PARAMETERS
        );
        assert!(verifier.is_null());
        assert!(last_error().contains("another"), "{}", last_error());

        prover = stale;
        assert_eq!(
            crop_proof_prover_load(ptr::null(), &mut prover),
            CROP_PROOF_ERROR_NULL
        );
        assert!(prover.is_null());
        assert_eq!(
            crop_proof_prover_load(path.as_ptr(), ptr::null_mut()),
            CROP_PROOF_ERROR_NULL
        );
        assert_eq!(
            crop_proof_verifier_load(c"\xff".as_ptr(), &mut verifier),
            CROP_PROOF_ERROR_ARGUMENT
        );
    }
}

/// A test binary links the crate as a Rust library, so rustc gives it the
/// crate's `#[global_allocator]` and no archive is involved. This shows that
/// the crate declares the allocator and that the probe sees it. Whether a
/// program that links the archive runs on it is for `c/check.sh` on the host
/// and for `NativeTests` in the app.
#[test]
fn large_allocations_are_mapped_on_apple_targets_only() {
    assert_eq!(
        crop_proof_large_allocations_are_mapped(),
        cfg!(target_vendor = "apple")
    );
}

/// The archive is the app's only Rust library, so it must carry the
/// fingerprint's C functions. Here they are reached through this crate.
#[test]
fn the_fingerprint_functions_are_part_of_the_library() {
    let name = unsafe { CStr::from_ptr(fingerprint_algorithm()) };
    assert_eq!(name.to_str().unwrap(), crop_proof::FINGERPRINT_ALGORITHM);
}

/// The header is written by hand; its constants and names must match the crate.
#[test]
fn header_matches_the_crate() {
    let define = |name: &str| -> usize {
        HEADER
            .lines()
            .find_map(|line| {
                let mut words = line.split_whitespace();
                (words.next() == Some("#define") && words.next() == Some(name))
                    .then(|| words.next().unwrap().parse().unwrap())
            })
            .unwrap_or_else(|| panic!("crop_proof.h does not define {name}"))
    };
    assert_eq!(define("CROP_PROOF_CAPACITY"), CROP_PROOF_CAPACITY);
    for (name, value) in [
        ("CROP_PROOF_OK", CROP_PROOF_OK),
        ("CROP_PROOF_ERROR_NULL", CROP_PROOF_ERROR_NULL),
        ("CROP_PROOF_ERROR_ARGUMENT", CROP_PROOF_ERROR_ARGUMENT),
        ("CROP_PROOF_ERROR_PARAMETERS", CROP_PROOF_ERROR_PARAMETERS),
        ("CROP_PROOF_ERROR_BUFFER", CROP_PROOF_ERROR_BUFFER),
        ("CROP_PROOF_ERROR_PROVE", CROP_PROOF_ERROR_PROVE),
        ("CROP_PROOF_ERROR_REJECTED", CROP_PROOF_ERROR_REJECTED),
        ("CROP_PROOF_ERROR_INTERNAL", CROP_PROOF_ERROR_INTERNAL),
    ] {
        assert_eq!(define(name), value as usize, "{name}");
    }
    for function in [
        "crop_proof_large_allocations_are_mapped(void)",
        "crop_proof_last_error(void)",
        "crop_proof_prover_load(",
        "crop_proof_prover_free(",
        "crop_proof_verifier_load(",
        "crop_proof_verifier_free(",
        "crop_proof_prove(",
        "crop_proof_verify(",
    ] {
        assert!(HEADER.contains(function), "crop_proof.h lacks {function}");
    }
    assert!(
        HEADER.contains(&format!("1 to {} pixels", crop_proof::MAX_PIXELS)),
        "crop_proof.h states another pixel limit"
    );
}
