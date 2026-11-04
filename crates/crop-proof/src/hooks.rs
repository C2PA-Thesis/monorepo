//! Read-only hooks that expose what a third party can recompute from a
//! published proof. Compiled only under the `attack-hooks` feature and used by
//! the soundness attack tests. They never relax or change verification.

use anyhow::Result;

use crate::{crop, wire, Fingerprint, ProverParams, F};

/// The crop challenges the verifier will draw for this proof. A third party can
/// recompute them because neither the published image nor the fingerprint is
/// absorbed into the transcript before the draw.
pub fn crop_challenges(fingerprint: &Fingerprint, proof: &[u8]) -> Result<Vec<F>> {
    let hashes = fingerprint.elements()?;
    let proof = wire::decode(proof)?;
    crop::replay_crop_challenges(&hashes, &proof)
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
