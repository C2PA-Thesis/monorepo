#![allow(dead_code)]
//! Builds one honest proof (setup, original, left-half crop, fingerprint,
//! proof) the way `tests/round_trip.rs` does, so the attack tests start from a
//! genuine, accepted proof.

use std::array;

use crop_proof::{
    crop, prove, setup_with_rng, Fingerprint, ProverParams, Rect, RgbImage, VerifierParams,
    ORIGINAL,
};
use rand::{RngCore, SeedableRng};
use tempfile::TempDir;

pub struct Honest {
    // Kept alive so the parameter files outlive the loaded params.
    _dir: TempDir,
    pub prover: ProverParams,
    pub verifier: VerifierParams,
    pub original: RgbImage,
    pub crop: RgbImage,
    pub fingerprint: Fingerprint,
    pub proof: Vec<u8>,
}

pub fn build(seed: u64) -> Honest {
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    let dir = tempfile::tempdir().unwrap();
    setup_with_rng(dir.path(), &mut rng).unwrap();
    let prover = ProverParams::load(dir.path()).unwrap();
    let verifier = VerifierParams::load(dir.path()).unwrap();

    let channels = array::from_fn(|_| {
        let mut pixels = vec![0u8; ORIGINAL.pixels()];
        rng.fill_bytes(&mut pixels);
        pixels
    });
    let original = RgbImage::new(ORIGINAL, channels).unwrap();
    let crop = crop(&original, Rect::LEFT_HALF).unwrap();
    let fingerprint = Fingerprint::of(&original).unwrap();
    let proof = prove(&prover, &original, &crop, Rect::LEFT_HALF, &fingerprint).unwrap();

    Honest {
        _dir: dir,
        prover,
        verifier,
        original,
        crop,
        fingerprint,
        proof,
    }
}
