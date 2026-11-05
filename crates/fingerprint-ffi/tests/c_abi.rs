//! The C ABI through its exported symbols, against the digest pinned in
//! `fingerprint`.

use std::ptr;

use fingerprint::{ELEMENT_BYTES, HASH_ROWS, MAX_PIXELS, TEST_IMAGE_DIGEST, TEST_SIZE};
use fingerprint_ffi::*;
use sha2::{Digest, Sha256};

const HEADER: &str = include_str!("../../../ios/Frameworks/include/fingerprint.h");

fn test_planes() -> [Vec<u8>; 3] {
    let mut planes = [(); 3].map(|()| vec![0u8; TEST_SIZE.pixels()]);
    let [r, g, b] = &mut planes;
    let status =
        unsafe { fingerprint_test_image(r.as_mut_ptr(), g.as_mut_ptr(), b.as_mut_ptr(), r.len()) };
    assert_eq!(status, FINGERPRINT_OK);
    planes
}

fn fingerprint_of(planes: &[Vec<u8>; 3]) -> Vec<u8> {
    let mut out = vec![0u8; FINGERPRINT_BYTES];
    let [r, g, b] = planes;
    let status = unsafe {
        fingerprint_rgb(
            r.as_ptr(),
            g.as_ptr(),
            b.as_ptr(),
            r.len(),
            out.as_mut_ptr(),
        )
    };
    assert_eq!(status, FINGERPRINT_OK);
    out
}

/// `Fingerprint::of` gives the same digest (see the `fingerprint` tests), so
/// the bytes match it too. One fingerprint only: CI runs tests unoptimized.
#[test]
fn test_image_matches_the_pinned_digest() {
    let planes = test_planes();
    assert_eq!(
        planes[..],
        fingerprint::test_image(TEST_SIZE).channels()[..]
    );

    let whole = fingerprint_of(&planes);
    assert_eq!(hex::encode(Sha256::digest(&whole)), TEST_IMAGE_DIGEST);

    let blue = &planes[2];
    let mut rows = vec![0u8; 3 * ELEMENT_BYTES];
    let status =
        unsafe { fingerprint_hash_rows(blue.as_ptr(), blue.len(), 125, 128, rows.as_mut_ptr()) };
    assert_eq!(status, FINGERPRINT_OK);
    let start = (2 * HASH_ROWS + 125) * ELEMENT_BYTES;
    assert_eq!(rows, whole[start..start + rows.len()]);

    let status =
        unsafe { fingerprint_hash_rows(blue.as_ptr(), blue.len(), 7, 7, ptr::dangling_mut()) };
    assert_eq!(status, FINGERPRINT_OK);
}

#[test]
fn bad_arguments_return_errors() {
    let pixels = vec![0u8; MAX_PIXELS + 1];
    let mut out = vec![0u8; FINGERPRINT_BYTES];
    let (p, o) = (pixels.as_ptr(), out.as_mut_ptr());
    unsafe {
        assert_eq!(
            fingerprint_rgb(p, ptr::null(), p, pixels.len(), o),
            FINGERPRINT_ERROR_NULL
        );
        assert_eq!(
            fingerprint_rgb(p, p, p, pixels.len(), ptr::null_mut()),
            FINGERPRINT_ERROR_NULL
        );
        assert_eq!(
            fingerprint_rgb(p, p, p, MAX_PIXELS + 1, o),
            FINGERPRINT_ERROR_LENGTH
        );
        assert_eq!(fingerprint_rgb(p, p, p, 0, o), FINGERPRINT_ERROR_LENGTH);
        assert_eq!(
            fingerprint_hash_rows(p, MAX_PIXELS, 4, 3, o),
            FINGERPRINT_ERROR_ROWS
        );
        assert_eq!(
            fingerprint_hash_rows(p, MAX_PIXELS, 0, HASH_ROWS + 1, o),
            FINGERPRINT_ERROR_ROWS
        );
        assert_eq!(
            fingerprint_test_image(o, o, ptr::null_mut(), 3),
            FINGERPRINT_ERROR_NULL
        );
        assert_eq!(fingerprint_test_image(o, o, o, 0), FINGERPRINT_ERROR_LENGTH);
    }
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
            .unwrap_or_else(|| panic!("fingerprint.h does not define {name}"))
    };
    assert_eq!(define("FINGERPRINT_MAX_PIXELS"), MAX_PIXELS);
    assert_eq!(
        TEST_SIZE.pixels(),
        MAX_PIXELS,
        "the pinned digest is for MAX_PIXELS"
    );
    assert_eq!(define("FINGERPRINT_HASH_ROWS"), HASH_ROWS);
    assert_eq!(define("FINGERPRINT_ELEMENT_BYTES"), ELEMENT_BYTES);
    assert_eq!(define("FINGERPRINT_BYTES"), FINGERPRINT_BYTES);
    for (name, value) in [
        ("FINGERPRINT_OK", FINGERPRINT_OK),
        ("FINGERPRINT_ERROR_NULL", FINGERPRINT_ERROR_NULL),
        ("FINGERPRINT_ERROR_LENGTH", FINGERPRINT_ERROR_LENGTH),
        ("FINGERPRINT_ERROR_ROWS", FINGERPRINT_ERROR_ROWS),
        ("FINGERPRINT_ERROR_INTERNAL", FINGERPRINT_ERROR_INTERNAL),
    ] {
        assert_eq!(define(name), value as usize, "{name}");
    }
    for function in [
        "fingerprint_algorithm(void)",
        "fingerprint_rgb(",
        "fingerprint_hash_rows(",
        "fingerprint_test_image(",
    ] {
        assert!(HEADER.contains(function), "fingerprint.h lacks {function}");
    }
    assert!(HEADER.contains(TEST_IMAGE_DIGEST));
}
