use crop_proof::{
    prove, setup_with_rng, verify, Fingerprint, ProverParams, RgbImage, VerifierParams, ORIGINAL,
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
    let crop = original.left_half().unwrap();
    let fingerprint = Fingerprint::of(&original).unwrap();

    let proof = prove(&prover, &original, &crop, &fingerprint).unwrap();
    verify(&verifier, &crop, &fingerprint, &proof).unwrap();

    let mut pixels = crop.channels().clone();
    pixels[0][0] ^= 1;
    let changed_crop = RgbImage::new(crop.size(), pixels).unwrap();
    assert!(verify(&verifier, &changed_crop, &fingerprint, &proof).is_err());

    let mut other = fingerprint.clone();
    other.r.swap(0, 1);
    assert!(verify(&verifier, &crop, &other, &proof).is_err());
    assert!(prove(&prover, &original, &crop, &other).is_err());

    let mut flipped = proof.clone();
    *flipped.last_mut().unwrap() ^= 1;
    assert!(verify(&verifier, &crop, &fingerprint, &flipped).is_err());
}
