//! The HyperVerITAS lattice fingerprint: `A · pixels` over the BLS12-381
//! scalar field, 128 field elements per RGB channel of a 1024x512 image.
//!
//! Extracted from HyperVerITAS (<https://github.com/C2PA-Thesis/HyperVerITAS>
//! at 798554f, MIT, see LICENSE). The crop proof checks a published image
//! against this value, and the capture device signs it. Both sides must
//! compute it identically, so this crate builds natively for the prover and
//! for `wasm32-unknown-unknown` for the capture page (see `wasm.rs`).

use anyhow::{anyhow, ensure, Context, Result};
use ark_ff::{BigInteger, PrimeField, Zero};
use ark_std::UniformRand;
use rand_chacha::{rand_core::SeedableRng, ChaCha8Rng};
use serde::{Deserialize, Serialize};

mod image;
#[cfg(target_arch = "wasm32")]
mod wasm;

pub use image::{RgbImage, Size, ORIGINAL};

pub type F = ark_bls12_381::Fr;

pub const ALGORITHM: &str = "hyperveritas-ajtai-chacha8-v1";

/// Rows of the matrix A, so field elements per channel.
pub const HASH_ROWS: usize = 128;

/// Bytes of one field element in the fingerprint's binary form.
pub const ELEMENT_BYTES: usize = 32;

/// Public binding to the original. It is deterministic, so anyone holding a
/// candidate image can check whether it is the original.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fingerprint {
    pub algorithm: String,
    pub size: Size,
    #[serde(rename = "R")]
    pub r: Vec<String>,
    #[serde(rename = "G")]
    pub g: Vec<String>,
    #[serde(rename = "B")]
    pub b: Vec<String>,
}

impl Fingerprint {
    pub fn of(image: &RgbImage) -> Result<Self> {
        ensure!(
            image.size() == ORIGINAL,
            "fingerprints are defined for a {ORIGINAL} original, got {}",
            image.size()
        );
        let [r, g, b] = image
            .channels()
            .each_ref()
            .map(|channel| hash_channel(channel).iter().map(to_hex).collect());
        Ok(Self {
            algorithm: ALGORITHM.to_string(),
            size: ORIGINAL,
            r,
            g,
            b,
        })
    }

    /// The three channels as field elements, after checking the fingerprint's shape.
    pub fn elements(&self) -> Result<[Vec<F>; 3]> {
        ensure!(
            self.algorithm == ALGORITHM,
            "unsupported fingerprint algorithm {}",
            self.algorithm
        );
        ensure!(
            self.size == ORIGINAL,
            "fingerprint is for a {} original, expected {ORIGINAL}",
            self.size
        );
        let decode = |name: &str, values: &[String]| -> Result<Vec<F>> {
            ensure!(
                values.len() == HASH_ROWS,
                "fingerprint channel {name} has {} elements, expected {HASH_ROWS}",
                values.len()
            );
            values.iter().map(|value| from_hex(value)).collect()
        };
        Ok([
            decode("R", &self.r)?,
            decode("G", &self.g)?,
            decode("B", &self.b)?,
        ])
    }
}

/// Row `i` of A is the ChaCha8 stream seeded with `i`, so both sides rebuild
/// A instead of storing its 128 x 2^19 entries.
fn matrix_row(row: usize) -> ChaCha8Rng {
    ChaCha8Rng::seed_from_u64(row as u64)
}

fn hash_row(pixels: &[u8], row: usize) -> F {
    let mut entries = matrix_row(row);
    pixels.iter().fold(F::zero(), |sum, &pixel| {
        sum + F::rand(&mut entries) * F::from(pixel)
    })
}

fn hash_channel(pixels: &[u8]) -> Vec<F> {
    parallel_map(HASH_ROWS, |row| hash_row(pixels, row))
}

/// Rows `rows` of one channel's hash as big-endian 32-byte elements, for
/// callers that split the work themselves, like the page's web workers.
pub fn hash_rows_into(pixels: &[u8], rows: std::ops::Range<usize>, out: &mut [u8]) {
    for (slot, row) in out.chunks_exact_mut(ELEMENT_BYTES).zip(rows) {
        slot.copy_from_slice(&hash_row(pixels, row).into_bigint().to_bytes_be());
    }
}

/// `r^T A` for row weights `r`.
pub fn combine_rows(weights: &[F]) -> Vec<F> {
    // One accumulator per worker: a vector per row would need 128 x 2^19 elements.
    let workers = worker_count(weights.len());
    let rows_per_worker = weights.len().div_ceil(workers);
    let partial = parallel_map(workers, |worker| {
        let mut combined = vec![F::zero(); ORIGINAL.pixels()];
        let rows = worker * rows_per_worker..((worker + 1) * rows_per_worker).min(weights.len());
        for row in rows {
            let mut entries = matrix_row(row);
            for value in &mut combined {
                *value += F::rand(&mut entries) * weights[row];
            }
        }
        combined
    });
    partial
        .into_iter()
        .reduce(|mut total, part| {
            total
                .iter_mut()
                .zip(part)
                .for_each(|(sum, value)| *sum += value);
            total
        })
        .unwrap_or_else(|| vec![F::zero(); ORIGINAL.pixels()])
}

fn worker_count(tasks: usize) -> usize {
    std::thread::available_parallelism()
        .map_or(1, usize::from)
        .clamp(1, tasks.max(1))
}

/// `(0..count).map(task)` spread over the available cores, in order.
#[cfg(not(target_arch = "wasm32"))]
fn parallel_map<T: Send>(count: usize, task: impl Fn(usize) -> T + Sync) -> Vec<T> {
    let workers = worker_count(count);
    let per_worker = count.div_ceil(workers);
    std::thread::scope(|scope| {
        let task = &task;
        let handles: Vec<_> = (0..workers)
            .map(|worker| {
                scope.spawn(move || {
                    (worker * per_worker..((worker + 1) * per_worker).min(count))
                        .map(task)
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|handle| handle.join().expect("fingerprint worker panicked"))
            .collect()
    })
}

/// The browser has no threads inside one wasm instance; the page runs
/// several instances in web workers instead.
#[cfg(target_arch = "wasm32")]
fn parallel_map<T>(count: usize, task: impl Fn(usize) -> T) -> Vec<T> {
    (0..count).map(task).collect()
}

fn to_hex(value: &F) -> String {
    let bytes = value.into_bigint().to_bytes_be();
    format!("0x{:0>64}", hex::encode(bytes))
}

fn from_hex(value: &str) -> Result<F> {
    let digits = value
        .strip_prefix("0x")
        .filter(|digits| digits.len() == 64)
        .ok_or_else(|| anyhow!("field element {value:?} is not 0x followed by 64 hex digits"))?;
    let element =
        F::from_be_bytes_mod_order(&hex::decode(digits).context("field element is not hex")?);
    ensure!(
        to_hex(&element) == value,
        "field element {value} is not canonical"
    );
    Ok(element)
}

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};

    use super::*;

    #[test]
    fn rows_are_deterministic_and_distinct() {
        let pixels = [1, 2, 3, 4];
        let first = hash_channel(&pixels);
        assert_eq!(first.len(), HASH_ROWS);
        assert_eq!(first, hash_channel(&pixels));
        assert_ne!(first[0], first[1]);
    }

    #[test]
    fn hex_round_trips_and_rejects_noncanonical_values() {
        let value = F::from(255u64);
        assert_eq!(from_hex(&to_hex(&value)).unwrap(), value);
        let modulus = "0x73eda753299d7d483339d80809a1d80553bda402fffe5bfeffffffff00000001";
        assert!(from_hex(modulus).is_err());
        assert!(from_hex("0x01").is_err());
    }

    #[test]
    fn binary_rows_match_the_hex_fingerprint() {
        let image = image::test_image();
        let fingerprint = Fingerprint::of(&image).unwrap();
        let mut out = vec![0u8; 2 * ELEMENT_BYTES];
        hash_rows_into(&image.channels()[1], 5..7, &mut out);
        assert_eq!(format!("0x{}", hex::encode(&out[..32])), fingerprint.g[5]);
        assert_eq!(format!("0x{}", hex::encode(&out[32..])), fingerprint.g[6]);
    }

    /// Test vector shared with the capture page: the SHA-256 of all 384
    /// elements of the test image's fingerprint, channel by channel, in binary.
    #[test]
    fn test_image_fingerprint_digest() {
        let image = image::test_image();
        let mut out = vec![0u8; 3 * HASH_ROWS * ELEMENT_BYTES];
        for (channel, slot) in image
            .channels()
            .iter()
            .zip(out.chunks_exact_mut(HASH_ROWS * ELEMENT_BYTES))
        {
            hash_rows_into(channel, 0..HASH_ROWS, slot);
        }
        assert_eq!(
            hex::encode(Sha256::digest(&out)),
            "cfa3ff823a6ac9e044bf3ba042bb7f7b5fc50ac696ad86e439d03acc273f5083"
        );
    }
}
