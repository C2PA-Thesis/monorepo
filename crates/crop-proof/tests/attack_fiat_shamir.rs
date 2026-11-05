//! Security analysis, attack A2. The forger fits another image to the crop
//! challenges of the honest proof. Since the transcript starts from a hash of
//! the statement, published pixels included, every challenge the verifier
//! draws for that image changes, and the proof's first sumcheck already fails.

mod support;

use crop_proof::{
    forge::{substitute_crop, CROP, RECT},
    verify, RgbImage,
};
use support::honest;

#[test]
#[ignore = "proves over 2^19 pixels; run with --release --features attack-hooks -- --ignored"]
fn fiat_shamir_substituted_crop_is_rejected() {
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
    let forged = substitute_crop(&h.fingerprint, RECT, &h.proof, &h.crop, &target).unwrap();
    assert_ne!(forged, h.crop);

    let error = verify(&h.verifier, &forged, RECT, &h.fingerprint, &h.proof)
        .expect_err("the verifier accepted another image with the honest proof");
    println!("rejected: {error:#}");
}
