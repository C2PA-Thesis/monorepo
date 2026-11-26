//! The prover writes the same bytes as the reference prover it replaced.

use crop_proof::{
    edit, hooks::prove_reference, prove, setup_with_rng, Fingerprint, ProverParams, Rect, RgbImage,
    Size, Turn,
};
use rand::{RngCore, SeedableRng};

#[test]
#[ignore = "proves over 2^19 pixels six times, about 6 GB; run with --release --ignored"]
fn proof_bytes_match_the_reference_prover() {
    let mut rng = rand::rngs::StdRng::seed_from_u64(11);
    let params = tempfile::tempdir().unwrap();
    setup_with_rng(params.path(), &mut rng).unwrap();
    let prover = ProverParams::load(params.path()).unwrap();

    let cases = [
        // Fills the parameters exactly.
        (
            Size {
                width: 1024,
                height: 512,
            },
            Rect {
                x: 0,
                y: 0,
                width: 512,
                height: 512,
            },
            0,
        ),
        // Padded, with a rectangle off the corner, turned.
        (
            Size::fit(4032, 3024),
            Rect {
                x: 137,
                y: 61,
                width: 300,
                height: 200,
            },
            1,
        ),
        // Almost all padding.
        (
            Size {
                width: 37,
                height: 23,
            },
            Rect {
                x: 5,
                y: 2,
                width: 30,
                height: 21,
            },
            3,
        ),
    ];
    for (size, rect, quarters) in cases {
        let channels = std::array::from_fn(|_| {
            let mut pixels = vec![0u8; size.pixels()];
            rng.fill_bytes(&mut pixels);
            pixels
        });
        let original = RgbImage::new(size, channels).unwrap();
        let turn = Turn::new(quarters).unwrap();
        let published = edit(&original, rect, turn).unwrap();
        let fingerprint = Fingerprint::of(&original).unwrap();
        let proof = prove(&prover, &original, &published, rect, turn, &fingerprint).unwrap();
        let reference =
            prove_reference(&prover, &original, &published, rect, turn, &fingerprint).unwrap();
        assert!(
            proof == reference,
            "the proof differs from the reference for {size:?} {rect} turned {turn}"
        );
    }
}
