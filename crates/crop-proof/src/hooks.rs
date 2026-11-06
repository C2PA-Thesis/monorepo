//! Read-only hooks that expose what a third party can recompute from a
//! published proof. Compiled only under the `attack-hooks` feature and used by
//! the soundness attack tests. They never relax or change verification.

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
    wire::canonical_bytes(&crop::commit_channel(&params.pcs, pixels)?)
}
