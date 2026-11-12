//! Security analysis, attacks A2 and A3 end to end. A third party takes the
//! file `provenance demo` published, replaces the crop, re-signs the C2PA
//! manifest with the public sample key (B2) and gives it to the reader's
//! verifier. The A2 forgery is rejected by the crop proof check, since the
//! proof is bound to the published pixels. The A3 forgery is rejected by the
//! same check, since the range check now commits H before its challenges. Run
//! after `provenance setup` and `provenance demo`.

use std::{
    fs,
    path::{Path, PathBuf},
};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use crop_proof::{
    forge::{forge_crop_proof, substitute_crop},
    ProverParams, RgbImage, Size, Turn,
};
use provenance::{
    image,
    manifest::{self, Assertion},
    verify::{Check, Verdict},
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
fn gradient(size: Size) -> RgbImage {
    let channels = std::array::from_fn(|c| {
        (0..size.pixels())
            .map(|i| {
                let (x, y) = (i % size.width, i / size.width);
                [x / 2, y / 2, 128][c] as u8
            })
            .collect()
    });
    RgbImage::new(size, channels).unwrap()
}

/// Publishes `pixels` with `assertion` under `out/attacks/<name>/` and runs
/// the reader's four checks on it.
fn republish_and_verify(name: &str, pixels: &RgbImage, assertion: &Assertion) -> Verdict {
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
    workspace
        .verifier()
        .unwrap()
        .verify(&signed, None, &mut |_| {})
}

fn demo() -> (Assertion, RgbImage) {
    let signed = repository().join("out/signed.png");
    let assertion = manifest::read(&signed).expect("run `provenance demo` first");
    assert_eq!(
        assertion.turn,
        Turn::NONE,
        "the forgers handle crops without a turn"
    );
    (assertion, image::read_png(&signed).unwrap())
}

#[test]
#[ignore = "needs `provenance setup` and `provenance demo`; run with --release -- --ignored"]
fn substituted_crop_is_rejected_by_the_crop_proof() {
    let (assertion, published) = demo();
    let proof = BASE64.decode(&assertion.image_proof).unwrap();
    let forged = substitute_crop(
        &assertion.receipt.fingerprint,
        assertion.crop,
        &proof,
        &published,
        &gradient(assertion.crop.size()),
    )
    .unwrap();
    assert_ne!(forged, published);

    // Same receipt, same location proof, same crop proof: only the pixels change.
    let verdict = republish_and_verify("substituted-crop", &forged, &assertion);
    let rejected = verdict
        .rejected
        .expect("the reader accepted the substituted crop");
    assert_eq!(rejected.check, Check::Image, "{}", rejected.detail);
}

#[test]
#[ignore = "needs `provenance setup` and `provenance demo`; run with --release -- --ignored"]
fn forged_crop_proof_is_rejected_by_the_range_check() {
    let (mut assertion, published) = demo();
    let prover = ProverParams::load(&workspace().crop_params()).unwrap();
    let target = gradient(assertion.crop.size());
    assert_ne!(target, published);

    // The forger never sees the original: the rest of its image is gray.
    let size = assertion.receipt.fingerprint.size;
    let gray = RgbImage::new(size, std::array::from_fn(|_| vec![128; size.pixels()])).unwrap();
    let proof = forge_crop_proof(
        &prover,
        &assertion.receipt.fingerprint,
        assertion.crop,
        &gray,
        &target,
    )
    .unwrap();
    assertion.image_proof = BASE64.encode(proof);

    // If accepted, anyone could publish any picture as a crop of this capture.
    let verdict = republish_and_verify("forged-crop", &target, &assertion);
    let rejected = verdict
        .rejected
        .expect("the reader accepted the forged crop proof");
    assert_eq!(rejected.check, Check::Image, "{}", rejected.detail);
    assert!(
        rejected.detail.contains("range check"),
        "rejected for another reason: {}",
        rejected.detail
    );
}
