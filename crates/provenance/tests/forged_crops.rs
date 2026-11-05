//! Security analysis, attacks A2 and A3 end to end. A third party takes the
//! file `provenance demo` published, replaces the crop, re-signs the C2PA
//! manifest with the public sample key (B2) and gives it to the reader's
//! verifier. Both tests pass while the attacks work; each fix must make its
//! test fail. Run after `provenance setup` and `provenance demo`.

use std::{
    fs,
    path::{Path, PathBuf},
};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use crop_proof::{
    forge::{forge_crop_proof, substitute_crop},
    ProverParams, RgbImage, CROP, ORIGINAL,
};
use provenance::{
    image,
    manifest::{self, Assertion},
    Workspace,
};

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn workspace() -> Workspace {
    let home = std::env::var_os("PROVENANCE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| repository().join(".provenance"));
    Workspace::new(home)
}

/// A color gradient, unrelated to the demo photo.
fn gradient() -> RgbImage {
    let channels = std::array::from_fn(|c| {
        (0..CROP.pixels())
            .map(|i| {
                let (x, y) = (i % CROP.width, i / CROP.width);
                [x / 2, y / 2, 128][c] as u8
            })
            .collect()
    });
    RgbImage::new(CROP, channels).unwrap()
}

/// Publishes `pixels` with `assertion` under `out/attacks/<name>/` and runs
/// the reader's four checks on it.
fn republish_and_verify(name: &str, pixels: &RgbImage, assertion: &Assertion) {
    let workspace = workspace();
    let dir = repository().join("out/attacks").join(name);
    fs::create_dir_all(&dir).unwrap();
    let unsigned = dir.join("crop.png");
    image::write_png(pixels, &unsigned).unwrap();
    let signed = dir.join("signed.png");
    workspace
        .editor()
        .unwrap()
        .sign(&unsigned, assertion, &signed)
        .unwrap();

    assert_eq!(&image::read_png(&signed).unwrap(), pixels);
    let verdict = workspace.verifier().unwrap().verify(&signed, None);
    assert!(
        verdict.rejected.is_none(),
        "the reader rejected the forgery: {:?}",
        verdict.rejected
    );
    assert_eq!(verdict.passed.len(), 4);
}

fn demo() -> (Assertion, RgbImage) {
    let signed = repository().join("out/signed.png");
    let assertion = manifest::read(&signed).expect("run `provenance demo` first");
    (assertion, image::read_png(&signed).unwrap())
}

#[test]
#[ignore = "needs `provenance setup` and `provenance demo`; run with --release -- --ignored"]
fn substituted_crop_passes_every_reader_check() {
    let (assertion, published) = demo();
    let proof = BASE64.decode(&assertion.image_proof).unwrap();
    let forged = substitute_crop(
        &assertion.receipt.fingerprint,
        &proof,
        &published,
        &gradient(),
    )
    .unwrap();
    assert_ne!(forged, published);

    // Same receipt, same location proof, same crop proof: only the pixels change.
    republish_and_verify("substituted-crop", &forged, &assertion);
}

#[test]
#[ignore = "needs `provenance setup` and `provenance demo`; run with --release -- --ignored"]
fn forged_crop_proof_passes_every_reader_check() {
    let (mut assertion, published) = demo();
    let prover = ProverParams::load(&workspace().crop_params()).unwrap();
    let target = gradient();
    assert_ne!(target, published);

    // The forger never sees the original: the rest of its image is gray.
    let gray = RgbImage::new(
        ORIGINAL,
        std::array::from_fn(|_| vec![128; ORIGINAL.pixels()]),
    )
    .unwrap();
    let proof = forge_crop_proof(&prover, &assertion.receipt.fingerprint, &gray, &target).unwrap();
    assertion.image_proof = BASE64.encode(proof);

    republish_and_verify("forged-crop", &target, &assertion);
}
