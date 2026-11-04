//! The published proof leaks a deterministic commitment to the original image.
//!
//! The proof's first three commitments are `Pcs::commit` of the original's
//! channels under the public prover params. Anyone with a candidate original
//! can recompute them and check for an exact match, and the public fingerprint
//! is likewise reproduced only by the true original. So the "privacy" of the
//! un-published right half is only as strong as the attacker's ability to guess
//! the original, not a cryptographic hiding property.
//!
//! This test demonstrates the leak by PASSING.

mod support;

use crop_proof::{hooks, Fingerprint};
use support::honest;

#[test]
#[ignore = "proves over 2^19 pixels; run with: cargo test --release -p crop-proof --features attack-hooks -- --ignored"]
fn commitments_identify_the_original_image() {
    let h = honest::build(7);
    let published = hooks::image_commitments(&h.proof).unwrap();

    // Recommitting to the true original reproduces the proof's commitments.
    for (ch, channel) in h.original.channels().iter().enumerate() {
        let recomputed = hooks::channel_commitment(&h.prover, channel).unwrap();
        assert_eq!(
            recomputed, published[ch],
            "channel {ch}: recomputed commitment does not match the proof"
        );
    }

    // A candidate differing in a single pixel does not match.
    let mut tampered = h.original.channels()[0].clone();
    tampered[0] ^= 1;
    let tampered_commitment = hooks::channel_commitment(&h.prover, &tampered).unwrap();
    assert_ne!(
        tampered_commitment, published[0],
        "a one-pixel change must change the commitment"
    );

    // The public fingerprint is reproduced exactly by the true original.
    assert_eq!(Fingerprint::of(&h.original).unwrap(), h.fingerprint);
}
