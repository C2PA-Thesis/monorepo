use std::{collections::BTreeMap, fs, path::Path};

use anyhow::{ensure, Context, Result};
use ark_bls12_381::Bls12_381;
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use ark_std::rand::RngCore;
use rand::SeedableRng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subroutines::pcs::prelude::{
    MultilinearProverParam, MultilinearVerifierParam, PolynomialCommitmentScheme,
};

use crate::{iop::check, Pcs, NUM_VARS, PROTOCOL};

const METADATA: &str = "metadata.json";
const PROVER: &str = "prover.bin";
const VERIFIER: &str = "verifier.bin";

pub struct ProverParams {
    pub(crate) pcs: MultilinearProverParam<Bls12_381>,
}

pub struct VerifierParams {
    pub(crate) pcs: MultilinearVerifierParam<Bls12_381>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Metadata {
    protocol: String,
    num_vars: usize,
    files: BTreeMap<String, Digest256>,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Digest256 {
    sha256: String,
    bytes: usize,
}

impl Digest256 {
    fn of(bytes: &[u8]) -> Self {
        Self {
            sha256: hex::encode(Sha256::digest(bytes)),
            bytes: bytes.len(),
        }
    }
}

/// Single-party KZG setup. Whoever runs it could keep the trapdoor and forge
/// proofs, which is acceptable for local research runs only.
pub fn setup(dir: &Path) -> Result<()> {
    setup_with_rng(dir, &mut rand::rngs::StdRng::from_entropy())
}

/// Setup from a caller-chosen RNG. A seeded RNG makes the trapdoor public;
/// use it only to reproduce parameters in tests.
pub fn setup_with_rng(dir: &Path, rng: &mut impl RngCore) -> Result<()> {
    ensure!(
        !dir.join(METADATA).exists(),
        "{} already holds parameters; delete it to run setup again",
        dir.display()
    );
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    // Openings in the range check are over NUM_VARS + 1 variables.
    let srs = check(
        Pcs::gen_srs_for_testing(rng, NUM_VARS + 1),
        "generating the SRS",
    )?;
    let (prover, verifier) = check(
        Pcs::trim(&srs, None, Some(NUM_VARS + 1)),
        "trimming the SRS",
    )?;

    let mut files = BTreeMap::new();
    for (name, bytes) in [
        (PROVER, uncompressed(&prover)?),
        (VERIFIER, uncompressed(&verifier)?),
    ] {
        fs::write(dir.join(name), &bytes).with_context(|| format!("writing {name}"))?;
        files.insert(name.to_string(), Digest256::of(&bytes));
    }
    let metadata = Metadata {
        protocol: PROTOCOL.to_string(),
        num_vars: NUM_VARS,
        files,
    };
    fs::write(dir.join(METADATA), serde_json::to_vec_pretty(&metadata)?)
        .context("writing metadata.json")
}

impl ProverParams {
    pub fn load(dir: &Path) -> Result<Self> {
        Ok(Self {
            pcs: load_member(dir, PROVER)?,
        })
    }
}

impl VerifierParams {
    pub fn load(dir: &Path) -> Result<Self> {
        Ok(Self {
            pcs: load_member(dir, VERIFIER)?,
        })
    }
}

fn load_member<T: CanonicalDeserialize + CanonicalSerialize>(dir: &Path, name: &str) -> Result<T> {
    let metadata_path = dir.join(METADATA);
    let metadata: Metadata = serde_json::from_slice(
        &fs::read(&metadata_path)
            .with_context(|| format!("reading {}", metadata_path.display()))?,
    )
    .with_context(|| format!("parsing {}", metadata_path.display()))?;
    ensure!(
        metadata.protocol == PROTOCOL && metadata.num_vars == NUM_VARS,
        "{} holds {} parameters over {} variables, expected {PROTOCOL} over {NUM_VARS}",
        dir.display(),
        metadata.protocol,
        metadata.num_vars
    );
    let expected = metadata
        .files
        .get(name)
        .with_context(|| format!("metadata lists no {name}"))?;
    let bytes = fs::read(dir.join(name)).with_context(|| format!("reading {name}"))?;
    ensure!(
        Digest256::of(&bytes) == *expected,
        "{name} does not match the SHA-256 in metadata.json"
    );
    // Unchecked: validating every curve point took minutes, and the digest
    // above already pins the file to what setup wrote.
    let mut reader = bytes.as_slice();
    let value = T::deserialize_uncompressed_unchecked(&mut reader)
        .with_context(|| format!("malformed {name}"))?;
    ensure!(reader.is_empty(), "{name} has trailing bytes");
    Ok(value)
}

fn uncompressed<T: CanonicalSerialize>(value: &T) -> Result<Vec<u8>> {
    let mut bytes = Vec::with_capacity(value.uncompressed_size());
    value
        .serialize_uncompressed(&mut bytes)
        .context("serializing parameters")?;
    Ok(bytes)
}
