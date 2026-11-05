//! Security analysis, attack A3. These tests pass while the range check draws
//! its challenges before the sorted vector H is committed. The fix must make
//! `forged_crop_of_another_picture_is_accepted` fail.

use ark_bls12_381::Fr as F;
use ark_ff::UniformRand;
use crop_proof::{
    crop,
    forge::{forge_crop_proof, solve, sorted_vector_for_product, CROP, ORIGINAL, RECT},
    setup_with_rng, verify, Fingerprint, ProverParams, RgbImage, VerifierParams,
};
use rand::{RngCore, SeedableRng};

#[test]
#[ignore = "proves over 2^19 pixels; run with --release --features attack-hooks -- --ignored"]
fn forged_crop_of_another_picture_is_accepted() {
    let mut rng = rand::rngs::StdRng::seed_from_u64(7);
    let params = tempfile::tempdir().unwrap();
    setup_with_rng(params.path(), &mut rng).unwrap();
    let prover = ProverParams::load(params.path()).unwrap();
    let verifier = VerifierParams::load(params.path()).unwrap();

    // The honest capture: the device signed this fingerprint.
    let channels = std::array::from_fn(|_| {
        let mut pixels = vec![0u8; ORIGINAL.pixels()];
        rng.fill_bytes(&mut pixels);
        pixels
    });
    let original = RgbImage::new(ORIGINAL, channels).unwrap();
    let fingerprint = Fingerprint::of(&original).unwrap();

    // A picture that has nothing to do with the original: a color gradient.
    let gradient = std::array::from_fn(|c| {
        (0..CROP.pixels())
            .map(|i| {
                let (x, y) = (i % CROP.width, i / CROP.width);
                [x / 2, y / 2, 128][c] as u8
            })
            .collect()
    });
    let target = RgbImage::new(CROP, gradient).unwrap();
    assert_ne!(target, crop(&original, RECT).unwrap());

    // The forger needs only the public fingerprint. The rest of its working
    // image is plain gray, not the original.
    let gray = RgbImage::new(
        ORIGINAL,
        std::array::from_fn(|_| vec![128; ORIGINAL.pixels()]),
    )
    .unwrap();
    let forged = forge_crop_proof(&prover, &fingerprint, RECT, &gray, &target).unwrap();

    verify(&verifier, &target, RECT, &fingerprint, &forged)
        .expect("the verifier accepts the gradient as a crop of the signed original");
    assert!(verify(
        &verifier,
        &crop(&original, RECT).unwrap(),
        RECT,
        &fingerprint,
        &forged
    )
    .is_err());
}

#[test]
fn sorted_vector_can_fit_any_product() {
    let mut rng = rand::rngs::StdRng::seed_from_u64(1);
    for _ in 0..4 {
        let (target, offset, mix) = (F::rand(&mut rng), F::rand(&mut rng), F::rand(&mut rng));
        // Fails inside if the product of the forged factors misses the target.
        sorted_vector_for_product(target, offset, mix).unwrap();
    }
}

#[test]
fn solves_a_random_system() {
    let mut rng = rand::rngs::StdRng::seed_from_u64(2);
    let n = 16;
    let matrix: Vec<Vec<F>> = (0..n)
        .map(|_| (0..n).map(|_| F::rand(&mut rng)).collect())
        .collect();
    let x: Vec<F> = (0..n).map(|_| F::rand(&mut rng)).collect();
    let rhs: Vec<F> = matrix
        .iter()
        .map(|row| row.iter().zip(&x).map(|(a, b)| *a * b).sum())
        .collect();
    assert_eq!(solve(matrix, rhs).unwrap(), x);
}
