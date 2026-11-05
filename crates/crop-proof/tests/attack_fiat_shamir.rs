//! Security analysis, attack A2. Passes while the crop challenges are drawn
//! before the published image and the fingerprint are absorbed. The fix must
//! make `fiat_shamir_substituted_crop_is_accepted` fail.

mod support;

use crop_proof::{
    forge::{substitute_crop, CROP, RECT},
    verify, RgbImage,
};
use support::honest;

#[test]
#[ignore = "proves over 2^19 pixels; run with --release --features attack-hooks -- --ignored"]
fn fiat_shamir_substituted_crop_is_accepted() {
    let h = honest::build(7);
    verify(&h.verifier, &h.crop, RECT, &h.fingerprint, &h.proof).unwrap();

    // A smooth gradient, unrelated to the noise-like honest crop.
    let gradient = std::array::from_fn(|c| {
        (0..CROP.pixels())
            .map(|i| {
                let (x, y) = (i % CROP.width, i / CROP.width);
                ((x / 3 + y / 5 + c * 37) % 200 + 20) as u8
            })
            .collect()
    });
    let target = RgbImage::new(CROP, gradient).unwrap();
    let forged = substitute_crop(&h.fingerprint, &h.proof, &h.crop, &target).unwrap();

    let differing: usize = forged
        .channels()
        .iter()
        .zip(h.crop.channels())
        .map(|(a, b)| a.iter().zip(b).filter(|(x, y)| x != y).count())
        .sum();
    println!("{differing} of {} values differ", 3 * CROP.pixels());
    assert!(differing > 700_000, "only {differing} values differ");

    verify(&h.verifier, &forged, RECT, &h.fingerprint, &h.proof)
        .expect("the verifier accepts another image with the honest proof");
}
