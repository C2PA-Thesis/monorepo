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

pub use image::{test_image, RgbImage, Size, ORIGINAL};

pub type F = ark_bls12_381::Fr;

pub const ALGORITHM: &str = "hyperveritas-ajtai-chacha8-v1";

/// Rows of the matrix A, so field elements per channel.
pub const HASH_ROWS: usize = 128;

/// Bytes of one field element in the fingerprint's binary form.
pub const ELEMENT_BYTES: usize = 32;

/// Test vector shared with the capture page and the C ABI: the SHA-256 of all
/// 384 elements of `test_image`'s fingerprint, channel by channel, in binary.
pub const TEST_IMAGE_DIGEST: &str =
    "cfa3ff823a6ac9e044bf3ba042bb7f7b5fc50ac696ad86e439d03acc273f5083";

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
        let elements = hash_image(image.channels().each_ref().map(Vec::as_slice));
        let mut channels = elements.chunks_exact(HASH_ROWS);
        let [r, g, b] = std::array::from_fn(|_| {
            channels
                .next()
                .expect("hash_image returns three channels")
                .iter()
                .map(to_hex)
                .collect()
        });
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
pub fn matrix_row(row: usize) -> ChaCha8Rng {
    ChaCha8Rng::seed_from_u64(row as u64)
}

fn hash_row(pixels: &[u8], row: usize) -> F {
    // Summing the entries per pixel value first leaves 256 multiplications
    // instead of one per pixel, which halves the time. Same entries, same order.
    let mut entries = matrix_row(row);
    let mut sums = [F::zero(); 256];
    for &pixel in pixels {
        sums[usize::from(pixel)] += F::rand(&mut entries);
    }
    sums.iter()
        .zip(0u64..)
        .fold(F::zero(), |total, (sum, value)| {
            total + *sum * F::from(value)
        })
}

/// All three channels in one pool of 384 rows, so no core idles between channels.
fn hash_image(channels: [&[u8]; 3]) -> Vec<F> {
    parallel_map(3 * HASH_ROWS, |index| {
        hash_row(channels[index / HASH_ROWS], index % HASH_ROWS)
    })
}

/// One channel's hash, `A · pixels`.
pub fn hash_channel(pixels: &[u8]) -> Vec<F> {
    parallel_map(HASH_ROWS, |row| hash_row(pixels, row))
}

fn write_elements(elements: &[F], out: &mut [u8]) {
    assert_eq!(
        out.len(),
        elements.len() * ELEMENT_BYTES,
        "output buffer does not fit the elements"
    );
    for (slot, element) in out.chunks_exact_mut(ELEMENT_BYTES).zip(elements) {
        slot.copy_from_slice(&element.into_bigint().to_bytes_be());
    }
}

/// Rows `rows` of one channel's hash as big-endian 32-byte elements, for
/// callers that split the work themselves, like the page's web workers.
pub fn hash_rows_into(pixels: &[u8], rows: std::ops::Range<usize>, out: &mut [u8]) {
    let first = rows.start;
    let elements = parallel_map(rows.len(), |offset| hash_row(pixels, first + offset));
    write_elements(&elements, out);
}

/// The whole fingerprint in binary: R, G, then B, `HASH_ROWS` big-endian
/// 32-byte elements each, the same values `Fingerprint::of` writes in hex.
pub fn hash_image_into(channels: [&[u8]; 3], out: &mut [u8]) {
    write_elements(&hash_image(channels), out);
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
///
/// Workers take the next task as they finish one, rather than a fixed share:
/// Apple chips mix fast and slow cores, and a fixed split waits for the slow ones.
#[cfg(not(target_arch = "wasm32"))]
fn parallel_map<T: Send>(count: usize, task: impl Fn(usize) -> T + Sync) -> Vec<T> {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let next = AtomicUsize::new(0);
    let done = std::thread::scope(|scope| {
        let (task, next) = (&task, &next);
        let handles: Vec<_> = (0..worker_count(count))
            .map(|_| {
                scope.spawn(move || {
                    std::iter::from_fn(|| {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        (index < count).then(|| (index, task(index)))
                    })
                    .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|handle| handle.join().expect("fingerprint worker panicked"))
            .collect::<Vec<_>>()
    });
    let mut slots: Vec<Option<T>> = std::iter::repeat_with(|| None).take(count).collect();
    for (index, value) in done {
        slots[index] = Some(value);
    }
    slots
        .into_iter()
        .map(|slot| slot.expect("every task ran once"))
        .collect()
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

    fn hash_channel(pixels: &[u8]) -> Vec<F> {
        parallel_map(HASH_ROWS, |row| hash_row(pixels, row))
    }

    #[test]
    fn rows_are_deterministic_and_distinct() {
        let pixels = [1, 2, 3, 4];
        let first = hash_channel(&pixels);
        assert_eq!(first.len(), HASH_ROWS);
        assert_eq!(first, hash_channel(&pixels));
        assert_ne!(first[0], first[1]);
    }

    /// `hash_row` regroups the sum; it must still be `sum_i A[row][i] * pixel_i`.
    #[test]
    fn row_matches_the_definition() {
        let pixels: Vec<u8> = (0..4096u32).map(|i| (i * 31 % 251) as u8).collect();
        let mut entries = matrix_row(3);
        let expected = pixels.iter().fold(F::zero(), |sum, &pixel| {
            sum + F::rand(&mut entries) * F::from(pixel)
        });
        assert_eq!(hash_row(&pixels, 3), expected);
    }

    #[test]
    fn parallel_map_keeps_order() {
        let squares = parallel_map(1000, |index| index * index);
        assert_eq!(
            squares,
            (0..1000).map(|index| index * index).collect::<Vec<_>>()
        );
        assert!(parallel_map(0, |index| index).is_empty());
    }

    #[test]
    fn hex_round_trips_and_rejects_noncanonical_values() {
        let value = F::from(255u64);
        assert_eq!(from_hex(&to_hex(&value)).unwrap(), value);
        let modulus = "0x73eda753299d7d483339d80809a1d80553bda402fffe5bfeffffffff00000001";
        assert!(from_hex(modulus).is_err());
        assert!(from_hex("0x01").is_err());
    }

    /// `Fingerprint::of` and `hash_rows_into`, the page's path, against the
    /// pinned digest. One fingerprint only: CI runs these tests unoptimized.
    #[test]
    fn test_image_fingerprint_digest() {
        let image = image::test_image();
        let fingerprint = Fingerprint::of(&image).unwrap();
        let binary: Vec<u8> = [&fingerprint.r, &fingerprint.g, &fingerprint.b]
            .into_iter()
            .flatten()
            .flat_map(|element| hex::decode(&element[2..]).unwrap())
            .collect();
        assert_eq!(hex::encode(Sha256::digest(&binary)), TEST_IMAGE_DIGEST);

        let mut rows = vec![0u8; 2 * ELEMENT_BYTES];
        hash_rows_into(&image.channels()[1], 5..7, &mut rows);
        let start = (HASH_ROWS + 5) * ELEMENT_BYTES;
        assert_eq!(rows, binary[start..start + rows.len()]);
    }
}
