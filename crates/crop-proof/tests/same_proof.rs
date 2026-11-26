//! The prover writes the same bytes as the reference prover it replaced.

use crop_proof::{
    edit, hooks::prove_reference, prove, setup_with_rng, Fingerprint, ProverParams, Rect, RgbImage,
    Size, Turn,
};
use rand::{rngs::StdRng, RngCore, SeedableRng};

/// Makes the channels of an original of the given size.
type Pixels = fn(&mut StdRng, Size) -> [Vec<u8>; 3];

/// Every byte value about as often as any other.
fn noise(rng: &mut StdRng, size: Size) -> [Vec<u8>; 3] {
    std::array::from_fn(|_| {
        let mut pixels = vec![0u8; size.pixels()];
        rng.fill_bytes(&mut pixels);
        pixels
    })
}

/// One channel all 0, one all 255 and one in flat blocks an eighth of each
/// side. A flat area gives the MSM runs of equal scalars (the sums aside of
/// `lean/msm.rs`), and a channel of zeros gives the opening zero quotients.
fn flat(_: &mut StdRng, size: Size) -> [Vec<u8>; 3] {
    const VALUES: [u8; 8] = [0, 1, 2, 127, 128, 200, 254, 255];
    let block = |pixel: usize| {
        let column = pixel % size.width * 8 / size.width;
        let row = pixel / size.width * 8 / size.height;
        VALUES[(column + 3 * row) % 8]
    };
    [
        vec![0; size.pixels()],
        vec![255; size.pixels()],
        (0..size.pixels()).map(block).collect(),
    ]
}

#[test]
#[ignore = "proves over 2^19 pixels eight times, about 6 GB; run with --release --ignored"]
fn proof_bytes_match_the_reference_prover() {
    let mut rng = rand::rngs::StdRng::seed_from_u64(11);
    let params = tempfile::tempdir().unwrap();
    setup_with_rng(params.path(), &mut rng).unwrap();
    let prover = ProverParams::load(params.path()).unwrap();

    let cases: [(Size, Rect, u8, Pixels); 4] = [
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
            noise,
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
            noise,
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
            noise,
        ),
        // Flat, padded, with a rectangle across several blocks, turned.
        (
            Size {
                width: 800,
                height: 600,
            },
            Rect {
                x: 150,
                y: 110,
                width: 301,
                height: 203,
            },
            1,
            flat,
        ),
    ];
    for (size, rect, quarters, pixels) in cases {
        let original = RgbImage::new(size, pixels(&mut rng, size)).unwrap();
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
