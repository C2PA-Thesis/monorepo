use std::{collections::BTreeMap, fs, path::Path};

use anyhow::{ensure, Context, Result};
use ark_bls12_381::Bls12_381;
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use ark_std::rand::RngCore;
use rand::SeedableRng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subroutines::pcs::prelude::{MultilinearVerifierParam, PolynomialCommitmentScheme};

use crate::{iop::check, lean::srs::Srs, Pcs, NUM_VARS, PROTOCOL};

const METADATA: &str = "metadata.json";
pub(crate) const PROVER: &str = "prover.bin";
const VERIFIER: &str = "verifier.bin";

pub struct ProverParams {
    pub(crate) srs: Srs,
    /// The key deserialized for hyperplonk's own prover, the first time the
    /// reference prover or a forger asks for it.
    #[cfg(feature = "attack-hooks")]
    reference: std::sync::OnceLock<subroutines::pcs::prelude::MultilinearProverParam<Bls12_381>>,
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
        write_new(dir, name, &bytes)?;
        files.insert(name.to_string(), Digest256::of(&bytes));
    }
    let metadata = Metadata {
        protocol: PROTOCOL.to_string(),
        num_vars: NUM_VARS,
        files,
    };
    write_new(dir, METADATA, &serde_json::to_vec_pretty(&metadata)?)
}

/// Writes `bytes` to a new file and renames it to `name`. Writing over a file
/// of that name would change the bytes under a prover that has it mapped, and
/// a setup that stops halfway would leave half a file under the final name.
fn write_new(dir: &Path, name: &str, bytes: &[u8]) -> Result<()> {
    let partial = dir.join(format!("{name}.partial"));
    fs::write(&partial, bytes).with_context(|| format!("writing {name}"))?;
    fs::rename(&partial, dir.join(name)).with_context(|| format!("writing {name}"))
}

impl ProverParams {
    /// Maps the prover key instead of reading it: the file is 201 MB, and
    /// pages the system can reload from the file do not count as memory the
    /// process owns.
    ///
    /// The key file must not be modified while the `ProverParams` is alive:
    /// - If it is truncated, or the system fails to read it, the process is
    ///   killed with SIGBUS the next time the prover touches a missing page.
    /// - If it is rewritten in place, `prove` returns "the prover key changed
    ///   while proving" when a point no longer decodes, and otherwise a proof
    ///   that fails verification.
    ///
    /// Replacing the file by renaming a new one over it, as `setup` does,
    /// leaves the loaded key as it was.
    ///
    /// The SHA-256 is checked once, here. It is an integrity check against
    /// corruption, not authentication: whoever can write the directory can
    /// write metadata.json as well.
    pub fn load(dir: &Path) -> Result<Self> {
        let expected = expected_digest(dir, PROVER)?;
        let srs = Srs::open(&dir.join(PROVER))?;
        ensure!(
            Digest256::of(srs.bytes()) == expected,
            "{PROVER} does not match the SHA-256 in metadata.json"
        );
        // The digest only ties the file to metadata.json. The prover panics
        // on a key without the basis for the openings of the range check.
        ensure!(
            srs.num_vars() > NUM_VARS,
            "{PROVER} holds a key over {} variables, expected at least {}",
            srs.num_vars(),
            NUM_VARS + 1
        );
        Ok(Self {
            srs,
            #[cfg(feature = "attack-hooks")]
            reference: std::sync::OnceLock::new(),
        })
    }

    /// The key as hyperplonk's own prover takes it, in the heap.
    #[cfg(feature = "attack-hooks")]
    pub(crate) fn pcs(&self) -> &subroutines::pcs::prelude::MultilinearProverParam<Bls12_381> {
        self.reference.get_or_init(|| {
            CanonicalDeserialize::deserialize_uncompressed_unchecked(self.srs.bytes())
                .expect("load checked the key's structure")
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

/// The digest setup recorded for `name`, after checking that `dir` holds
/// parameters of this protocol and size.
fn expected_digest(dir: &Path, name: &str) -> Result<Digest256> {
    let metadata_path = dir.join(METADATA);
    let mut metadata: Metadata = serde_json::from_slice(
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
    metadata
        .files
        .remove(name)
        .with_context(|| format!("metadata lists no {name}"))
}

fn load_member<T: CanonicalDeserialize + CanonicalSerialize>(dir: &Path, name: &str) -> Result<T> {
    let expected = expected_digest(dir, name)?;
    let bytes = fs::read(dir.join(name)).with_context(|| format!("reading {name}"))?;
    ensure!(
        Digest256::of(&bytes) == expected,
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

#[cfg(test)]
mod tests {
    use std::io::Read;

    use tempfile::TempDir;

    use super::*;
    use crate::lean::srs::tests as srs;

    /// A parameter directory with `key` as its prover key and a metadata.json
    /// that records `recorded` for it.
    fn directory(key: &[u8], recorded: Digest256) -> TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(PROVER), key).unwrap();
        let metadata = Metadata {
            protocol: PROTOCOL.to_string(),
            num_vars: NUM_VARS,
            files: BTreeMap::from([(PROVER.to_string(), recorded)]),
        };
        let metadata = serde_json::to_vec_pretty(&metadata).unwrap();
        fs::write(dir.path().join(METADATA), metadata).unwrap();
        dir
    }

    fn load_error(dir: &TempDir) -> String {
        match ProverParams::load(dir.path()) {
            Ok(_) => panic!("the parameters were loaded"),
            Err(error) => error.to_string(),
        }
    }

    #[test]
    fn a_key_over_too_few_variables_is_refused_when_loaded() {
        // A well-formed key that metadata.json describes exactly, so only
        // its size is wrong. The prover used to panic on it.
        let key = uncompressed(&srs::params(4)).unwrap();
        let dir = directory(&key, Digest256::of(&key));
        let error = load_error(&dir);
        assert!(error.contains("a key over 4 variables"), "{error}");
    }

    #[test]
    fn a_key_that_differs_from_its_digest_is_refused_when_loaded() {
        let key = uncompressed(&srs::params(4)).unwrap();
        let mut recorded = key.clone();
        *recorded.last_mut().unwrap() ^= 1;
        let dir = directory(&key, Digest256::of(&recorded));
        let error = load_error(&dir);
        assert!(error.contains("SHA-256"), "{error}");
    }

    fn verifier_error(dir: &TempDir) -> String {
        match VerifierParams::load(dir.path()) {
            Ok(_) => panic!("the parameters were loaded"),
            Err(error) => error.to_string(),
        }
    }

    /// A parameter directory whose metadata.json records `recorded` for the
    /// verifier key `key`.
    fn verifier_directory(key: &[u8], recorded: Digest256) -> TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(VERIFIER), key).unwrap();
        let metadata = Metadata {
            protocol: PROTOCOL.to_string(),
            num_vars: NUM_VARS,
            files: BTreeMap::from([(VERIFIER.to_string(), recorded)]),
        };
        let metadata = serde_json::to_vec_pretty(&metadata).unwrap();
        fs::write(dir.path().join(METADATA), metadata).unwrap();
        dir
    }

    #[test]
    fn a_verifier_key_that_differs_from_its_digest_is_refused_when_loaded() {
        // The digest is compared before the bytes are parsed, so any bytes do.
        let key = b"a verifier key";
        let dir = verifier_directory(key, Digest256::of(b"another verifier key"));
        let error = verifier_error(&dir);
        assert!(error.contains("SHA-256"), "{error}");
    }

    #[test]
    fn a_verifier_key_that_matches_its_digest_gets_past_the_digest() {
        // Bytes that are no key: the refusal now comes from parsing them.
        let key = b"a verifier key";
        let dir = verifier_directory(key, Digest256::of(key));
        let error = verifier_error(&dir);
        assert!(error.contains("malformed"), "{error}");
    }

    #[test]
    fn a_verifier_key_that_metadata_does_not_list_is_refused_when_loaded() {
        let key = uncompressed(&srs::params(4)).unwrap();
        let dir = directory(&key, Digest256::of(&key));
        fs::write(dir.path().join(VERIFIER), b"a verifier key").unwrap();
        let error = verifier_error(&dir);
        assert!(error.contains("metadata lists no"), "{error}");
    }

    #[test]
    fn a_member_written_again_is_a_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(PROVER);
        write_new(dir.path(), PROVER, b"the first key").unwrap();
        // Stands for a prover that has the first file mapped.
        let mut held = fs::File::open(&path).unwrap();
        write_new(dir.path(), PROVER, b"another").unwrap();

        let mut first = Vec::new();
        held.read_to_end(&mut first).unwrap();
        assert_eq!(first, b"the first key");
        assert_eq!(fs::read(&path).unwrap(), b"another");
        let names: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, [PROVER]);
    }
}
