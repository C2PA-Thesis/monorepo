//! Read-only hooks that expose what a third party can recompute from a
//! published proof, and the reference prover, which takes the private
//! original as `prove` does. Compiled only under the `attack-hooks` feature
//! and used by the soundness attack tests and the comparison of the two
//! provers. They never relax or change verification.

use anyhow::Result;

use crate::{crop, wire, Fingerprint, ProverParams, Rect, RgbImage, Turn, F};

/// The crop challenges the verifier draws for this proof, rectangle, turn and
/// published image. The statement opens the transcript, so another published
/// image gets other challenges.
pub fn crop_challenges(
    fingerprint: &Fingerprint,
    rect: Rect,
    turn: Turn,
    published: &RgbImage,
    proof: &[u8],
) -> Result<Vec<F>> {
    let hashes = fingerprint.elements()?;
    let proof = wire::decode(proof)?;
    crop::replay_crop_challenges(rect, turn, fingerprint.size, &hashes, published, &proof)
}

/// The proof's first three commitments (the original image's channels), as
/// canonical bytes. These are a deterministic commitment to the original.
pub fn image_commitments(proof: &[u8]) -> Result<[Vec<u8>; 3]> {
    let proof = wire::decode(proof)?;
    let mut out = std::array::from_fn(|_| Vec::new());
    for (slot, commitment) in out.iter_mut().zip(&proof.commitments[..3]) {
        *slot = wire::canonical_bytes(commitment)?;
    }
    Ok(out)
}

/// Commits to one channel with the public prover params, the same way the
/// prover does, so a candidate image can be matched against the proof's
/// commitments byte for byte.
pub fn channel_commitment(params: &ProverParams, pixels: &[u8]) -> Result<Vec<u8>> {
    wire::canonical_bytes(&crop::commit_channel(params, pixels)?)
}

/// The proof the reference prover writes for the same inputs as `prove`. The
/// two must agree byte for byte. The reference prover needs about 6 GB.
pub fn prove_reference(
    params: &ProverParams,
    original: &RgbImage,
    published: &RgbImage,
    rect: Rect,
    turn: Turn,
    fingerprint: &Fingerprint,
) -> Result<Vec<u8>> {
    crop::prove_reference(params, original, published, rect, turn, fingerprint)
}
