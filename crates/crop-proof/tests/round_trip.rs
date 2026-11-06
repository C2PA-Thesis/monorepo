use crop_proof::{
    crop, edit, prove, setup_with_rng, verify, Fingerprint, ProverParams, Rect, RgbImage, Size,
    Turn, VerifierParams,
};
use rand::{RngCore, SeedableRng};

#[test]
#[ignore = "proves over 2^19 pixels, about half a minute; run with --release --ignored"]
fn proves_verifies_and_rejects_tampering() {
    let mut rng = rand::rngs::StdRng::seed_from_u64(7);
    let params = tempfile::tempdir().unwrap();
    setup_with_rng(params.path(), &mut rng).unwrap();
    let prover = ProverParams::load(params.path()).unwrap();
    let verifier = VerifierParams::load(params.path()).unwrap();

    // The 4:3 original of a 12 MP photo: not a power of two, so the
    // polynomials are padded.
    let size = Size::fit(4032, 3024);
    let channels = std::array::from_fn(|_| {
        let mut pixels = vec![0u8; size.pixels()];
        rng.fill_bytes(&mut pixels);
        pixels
    });
    let original = RgbImage::new(size, channels).unwrap();
    // Not the left half, and not a power-of-two size.
    let rect = Rect {
        x: 137,
        y: 61,
        width: 300,
        height: 200,
    };
    let published = crop(&original, rect).unwrap();
    let fingerprint = Fingerprint::of(&original).unwrap();

    let proof = prove(
        &prover,
        &original,
        &published,
        rect,
        Turn::NONE,
        &fingerprint,
    )
    .unwrap();
    verify(
        &verifier,
        &published,
        rect,
        Turn::NONE,
        &fingerprint,
        &proof,
    )
    .unwrap();

    let mut pixels = published.channels().clone();
    pixels[0][0] ^= 1;
    let changed = RgbImage::new(published.size(), pixels).unwrap();
    assert!(verify(&verifier, &changed, rect, Turn::NONE, &fingerprint, &proof).is_err());

    let shifted = Rect {
        x: rect.x + 1,
        ..rect
    };
    assert!(verify(
        &verifier,
        &published,
        shifted,
        Turn::NONE,
        &fingerprint,
        &proof
    )
    .is_err());
    assert!(prove(
        &prover,
        &original,
        &published,
        shifted,
        Turn::NONE,
        &fingerprint
    )
    .is_err());

    // Same elements, another size: one row fewer, and the transposed size,
    // which reads the same bytes. The rectangle fits both.
    for other_size in [
        Size {
            height: size.height - 1,
            ..size
        },
        Size {
            width: size.height,
            height: size.width,
        },
    ] {
        let other = Fingerprint {
            size: other_size,
            ..fingerprint.clone()
        };
        assert!(verify(&verifier, &published, rect, Turn::NONE, &other, &proof).is_err());
    }

    let mut other = fingerprint.clone();
    other.r.swap(0, 1);
    assert!(verify(&verifier, &published, rect, Turn::NONE, &other, &proof).is_err());
    assert!(prove(&prover, &original, &published, rect, Turn::NONE, &other).is_err());

    let mut flipped = proof.clone();
    *flipped.last_mut().unwrap() ^= 1;
    assert!(verify(
        &verifier,
        &published,
        rect,
        Turn::NONE,
        &fingerprint,
        &flipped
    )
    .is_err());

    // A quarter turn: the published image is 200x300, and the proof binds
    // the turn as well as the rectangle.
    let quarter = Turn::new(1).unwrap();
    let turned = edit(&original, rect, quarter).unwrap();
    let proof = prove(&prover, &original, &turned, rect, quarter, &fingerprint).unwrap();
    verify(&verifier, &turned, rect, quarter, &fingerprint, &proof).unwrap();
    // The other way round has the same size, so only the binding catches it.
    let three_quarters = Turn::new(3).unwrap();
    assert!(verify(
        &verifier,
        &turned,
        rect,
        three_quarters,
        &fingerprint,
        &proof
    )
    .is_err());
    let backwards = edit(&original, rect, three_quarters).unwrap();
    assert!(verify(&verifier, &backwards, rect, quarter, &fingerprint, &proof).is_err());
    assert!(verify(
        &verifier,
        &backwards,
        rect,
        three_quarters,
        &fingerprint,
        &proof
    )
    .is_err());
    assert!(prove(
        &prover,
        &original,
        &turned,
        rect,
        three_quarters,
        &fingerprint
    )
    .is_err());
    // Unturned pixels under the turned claim, and the turned ones without it.
    assert!(verify(&verifier, &published, rect, quarter, &fingerprint, &proof).is_err());
    assert!(verify(&verifier, &turned, rect, Turn::NONE, &fingerprint, &proof).is_err());
}
