use crop_proof::{
    crop, prove, setup_with_rng, verify, Fingerprint, ProverParams, Rect, RgbImage, VerifierParams,
    ORIGINAL,
};
use rand::{RngCore, SeedableRng};

#[test]
#[ignore = "proves over 2^19 pixels, about three minutes; run with --release --ignored"]
fn proves_verifies_and_rejects_tampering() {
    let mut rng = rand::rngs::StdRng::seed_from_u64(7);
    let params = tempfile::tempdir().unwrap();
    setup_with_rng(params.path(), &mut rng).unwrap();
    let prover = ProverParams::load(params.path()).unwrap();
    let verifier = VerifierParams::load(params.path()).unwrap();

    let channels = std::array::from_fn(|_| {
        let mut pixels = vec![0u8; ORIGINAL.pixels()];
        rng.fill_bytes(&mut pixels);
        pixels
    });
    let original = RgbImage::new(ORIGINAL, channels).unwrap();
    // Not the left half, and not a power-of-two size.
    let rect = Rect {
        x: 137,
        y: 61,
        width: 300,
        height: 200,
    };
    let published = crop(&original, rect).unwrap();
    let fingerprint = Fingerprint::of(&original).unwrap();

    let proof = prove(&prover, &original, &published, rect, &fingerprint).unwrap();
    verify(&verifier, &published, rect, &fingerprint, &proof).unwrap();

    let mut pixels = published.channels().clone();
    pixels[0][0] ^= 1;
    let changed = RgbImage::new(published.size(), pixels).unwrap();
    assert!(verify(&verifier, &changed, rect, &fingerprint, &proof).is_err());

    let shifted = Rect {
        x: rect.x + 1,
        ..rect
    };
    assert!(verify(&verifier, &published, shifted, &fingerprint, &proof).is_err());
    assert!(prove(&prover, &original, &published, shifted, &fingerprint).is_err());

    let mut other = fingerprint.clone();
    other.r.swap(0, 1);
    assert!(verify(&verifier, &published, rect, &other, &proof).is_err());
    assert!(prove(&prover, &original, &published, rect, &other).is_err());

    let mut flipped = proof.clone();
    *flipped.last_mut().unwrap() ^= 1;
    assert!(verify(&verifier, &published, rect, &fingerprint, &flipped).is_err());
}
