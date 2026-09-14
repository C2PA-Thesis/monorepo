//! Proof encoding: a version magic, then arkworks canonical members, each
//! prefixed by its length. Kept byte-compatible with the HyperVerITAS fork.

use std::io::{Cursor, Read};

use anyhow::{ensure, Context, Result};
use ark_bls12_381::Bls12_381;
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use subroutines::{
    pcs::prelude::{Commitment, MultilinearKzgProof},
    poly_iop::prelude::IOPProof,
    BatchProof,
};

use crate::{Pcs, F, NUM_VARS};

const MAGIC: &[u8; 8] = b"HVPST001";
const MAX_PROOF_BYTES: usize = 16 * 1024 * 1024;
const MAX_MEMBER_BYTES: usize = 8 * 1024 * 1024;
/// Sumcheck round polynomials in this proof have degree at most 3.
const MAX_ROUND_EVALUATIONS: usize = 8;

pub(crate) struct CropProof {
    /// Image channels, then (counts, product, fraction) per channel's range check.
    pub commitments: Vec<Commitment<Bls12_381>>,
    pub hash: [Sumcheck; 3],
    pub range: [Sumcheck; 3],
    pub crop: [Sumcheck; 3],
    pub image_openings: BatchOpening,
    pub range_openings: BatchOpening,
}

/// What the verifier reads from a sumcheck: its challenge point and each
/// round's evaluations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Sumcheck {
    pub point: Vec<F>,
    pub rounds: Vec<Vec<F>>,
}

pub(crate) struct BatchOpening {
    pub sumcheck: Sumcheck,
    pub evaluations: Vec<F>,
    pub kzg: MultilinearKzgProof<Bls12_381>,
}

impl From<IOPProof<F>> for Sumcheck {
    fn from(proof: IOPProof<F>) -> Self {
        Self {
            point: proof.point,
            rounds: proof
                .proofs
                .into_iter()
                .map(|message| message.evaluations)
                .collect(),
        }
    }
}

impl From<BatchProof<Bls12_381, Pcs>> for BatchOpening {
    fn from(proof: BatchProof<Bls12_381, Pcs>) -> Self {
        Self {
            sumcheck: proof.sum_check_proof.into(),
            evaluations: proof.f_i_eval_at_point_i,
            kzg: proof.g_prime_proof,
        }
    }
}

pub(crate) fn encode(proof: &CropProof) -> Result<Vec<u8>> {
    let mut out = MAGIC.to_vec();
    write_frame(&mut out, &canonical_bytes(&proof.commitments)?)?;
    for sumcheck in proof.hash.iter().chain(&proof.range).chain(&proof.crop) {
        write_sumcheck(&mut out, sumcheck)?;
    }
    for opening in [&proof.image_openings, &proof.range_openings] {
        write_sumcheck(&mut out, &opening.sumcheck)?;
        write_frame(&mut out, &canonical_bytes(&opening.evaluations)?)?;
        write_frame(&mut out, &canonical_bytes(&opening.kzg)?)?;
    }
    Ok(out)
}

pub(crate) fn decode(bytes: &[u8]) -> Result<CropProof> {
    ensure!(
        bytes.len() <= MAX_PROOF_BYTES,
        "image proof is larger than {MAX_PROOF_BYTES} bytes"
    );
    let body = bytes
        .strip_prefix(MAGIC.as_slice())
        .context("image proof does not start with HVPST001")?;
    let mut cursor = Cursor::new(body);
    let commitments =
        from_canonical_bytes(&read_frame(&mut cursor, "commitments")?, "commitments")?;
    let mut sumchecks = Vec::with_capacity(9);
    for label in ["hash", "range", "crop"] {
        for channel in 0..3 {
            sumchecks.push(read_sumcheck(
                &mut cursor,
                &format!("{label} sumcheck {channel}"),
            )?);
        }
    }
    let mut openings = Vec::with_capacity(2);
    for label in ["image openings", "range openings"] {
        openings.push(BatchOpening {
            sumcheck: read_sumcheck(&mut cursor, label)?,
            evaluations: from_canonical_bytes(&read_frame(&mut cursor, label)?, label)?,
            kzg: from_canonical_bytes(&read_frame(&mut cursor, label)?, label)?,
        });
    }
    ensure!(
        cursor.position() as usize == body.len(),
        "image proof has trailing bytes"
    );

    let mut sumchecks = sumchecks.into_iter();
    let mut next_three =
        || -> [Sumcheck; 3] { std::array::from_fn(|_| sumchecks.next().expect("nine sumchecks")) };
    let (hash, range, crop) = (next_three(), next_three(), next_three());
    let range_openings = openings.pop().expect("two openings");
    let image_openings = openings.pop().expect("two openings");
    let proof = CropProof {
        commitments,
        hash,
        range,
        crop,
        image_openings,
        range_openings,
    };
    check_shape(&proof)?;
    Ok(proof)
}

fn check_shape(proof: &CropProof) -> Result<()> {
    ensure!(
        proof.commitments.len() == 12,
        "image proof must carry 12 commitments"
    );
    let range_vars = NUM_VARS + 1;
    for (label, sumchecks, vars) in [
        ("hash", &proof.hash, NUM_VARS),
        ("range", &proof.range, range_vars),
        ("crop", &proof.crop, NUM_VARS),
    ] {
        for sumcheck in sumchecks {
            check_sumcheck_shape(sumcheck, vars, label)?;
        }
    }
    check_sumcheck_shape(&proof.image_openings.sumcheck, NUM_VARS, "image openings")?;
    check_sumcheck_shape(&proof.range_openings.sumcheck, range_vars, "range openings")?;
    ensure!(
        proof.image_openings.evaluations.len() == 9,
        "image openings must carry 9 evaluations"
    );
    ensure!(
        proof.range_openings.evaluations.len() == 33,
        "range openings must carry 33 evaluations"
    );
    Ok(())
}

fn check_sumcheck_shape(sumcheck: &Sumcheck, vars: usize, label: &str) -> Result<()> {
    ensure!(
        sumcheck.point.len() == vars && sumcheck.rounds.len() == vars,
        "{label} sumcheck must have {vars} rounds"
    );
    ensure!(
        sumcheck
            .rounds
            .iter()
            .all(|round| (1..=MAX_ROUND_EVALUATIONS).contains(&round.len())),
        "{label} sumcheck has a malformed round"
    );
    Ok(())
}

pub(crate) fn canonical_bytes<T: CanonicalSerialize>(value: &T) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    value
        .serialize_compressed(&mut bytes)
        .context("serializing a proof member")?;
    Ok(bytes)
}

pub(crate) fn from_canonical_bytes<T: CanonicalDeserialize>(
    bytes: &[u8],
    label: &str,
) -> Result<T> {
    let mut cursor = Cursor::new(bytes);
    let value =
        T::deserialize_compressed(&mut cursor).with_context(|| format!("malformed {label}"))?;
    ensure!(
        cursor.position() as usize == bytes.len(),
        "{label} has trailing bytes"
    );
    Ok(value)
}

fn write_frame(out: &mut Vec<u8>, bytes: &[u8]) -> Result<()> {
    out.extend_from_slice(&u64::try_from(bytes.len())?.to_le_bytes());
    out.extend_from_slice(bytes);
    Ok(())
}

fn read_frame(cursor: &mut Cursor<&[u8]>, label: &str) -> Result<Vec<u8>> {
    let mut length = [0u8; 8];
    cursor
        .read_exact(&mut length)
        .with_context(|| format!("{label} is truncated"))?;
    let length = usize::try_from(u64::from_le_bytes(length))?;
    ensure!(
        length <= MAX_MEMBER_BYTES,
        "{label} is larger than {MAX_MEMBER_BYTES} bytes"
    );
    let mut bytes = vec![0u8; length];
    cursor
        .read_exact(&mut bytes)
        .with_context(|| format!("{label} is truncated"))?;
    Ok(bytes)
}

fn write_sumcheck(out: &mut Vec<u8>, sumcheck: &Sumcheck) -> Result<()> {
    write_frame(out, &canonical_bytes(&sumcheck.point)?)?;
    out.extend_from_slice(&u32::try_from(sumcheck.rounds.len())?.to_le_bytes());
    for round in &sumcheck.rounds {
        write_frame(out, &canonical_bytes(round)?)?;
    }
    Ok(())
}

fn read_sumcheck(cursor: &mut Cursor<&[u8]>, label: &str) -> Result<Sumcheck> {
    let point = from_canonical_bytes(&read_frame(cursor, label)?, label)?;
    let mut count = [0u8; 4];
    cursor
        .read_exact(&mut count)
        .with_context(|| format!("{label} is truncated"))?;
    let count = u32::from_le_bytes(count) as usize;
    ensure!(count <= 64, "{label} has {count} rounds");
    let rounds = (0..count)
        .map(|_| from_canonical_bytes(&read_frame(cursor, label)?, label))
        .collect::<Result<_>>()?;
    Ok(Sumcheck { point, rounds })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ark_ff::Zero;

    fn sumcheck(vars: usize, evaluations: usize) -> Sumcheck {
        Sumcheck {
            point: vec![F::zero(); vars],
            rounds: vec![vec![F::zero(); evaluations]; vars],
        }
    }

    fn synthetic() -> CropProof {
        let opening = |vars, evaluations| BatchOpening {
            sumcheck: sumcheck(vars, 3),
            evaluations: vec![F::zero(); evaluations],
            kzg: MultilinearKzgProof { proofs: Vec::new() },
        };
        CropProof {
            commitments: vec![Commitment::default(); 12],
            hash: std::array::from_fn(|_| sumcheck(19, 3)),
            range: std::array::from_fn(|_| sumcheck(20, 4)),
            crop: std::array::from_fn(|_| sumcheck(19, 3)),
            image_openings: opening(19, 9),
            range_openings: opening(20, 33),
        }
    }

    #[test]
    fn encoding_round_trips() {
        let bytes = encode(&synthetic()).unwrap();
        assert_eq!(encode(&decode(&bytes).unwrap()).unwrap(), bytes);
    }

    #[test]
    fn rejects_garbage_truncation_and_trailing_bytes() {
        let bytes = encode(&synthetic()).unwrap();
        assert!(decode(b"not a proof").is_err());
        assert!(decode(&bytes[..bytes.len() - 1]).is_err());
        assert!(decode(&[bytes.as_slice(), &[0]].concat()).is_err());
    }
}
