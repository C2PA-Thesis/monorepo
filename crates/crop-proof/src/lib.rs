//! HyperVerITAS PST proof that a published image is the left half of a
//! fingerprinted original.
//!
//! Extracted from HyperVerITAS (<https://github.com/C2PA-Thesis/HyperVerITAS>
//! at 798554f, MIT, see LICENSE). The statement is fixed to a 1024x512
//! original and its 512x512 left half, which is what the thesis PoC publishes.
//! The fingerprint itself lives in the `fingerprint` crate, shared with the
//! capture device.

use anyhow::{ensure, Result};

mod crop;
mod iop;
mod params;
mod wire;

pub use crop::{prove, verify};
pub use fingerprint::{Fingerprint, RgbImage, Size, ALGORITHM as FINGERPRINT_ALGORITHM, ORIGINAL};
pub use params::{setup, setup_with_rng, ProverParams, VerifierParams};

pub const PROTOCOL: &str = "hyperveritas-pst-crop-v1";
pub const CROP: Size = Size {
    width: 512,
    height: 512,
};

/// log2 of the original's pixel count, so the number of variables of every
/// polynomial over the original.
const NUM_VARS: usize = 19;

type F = fingerprint::F;
type Pcs = subroutines::pcs::prelude::MultilinearKzgPCS<ark_bls12_381::Bls12_381>;

/// The only edit this crate proves.
pub fn left_half(original: &RgbImage) -> Result<RgbImage> {
    ensure!(
        original.size() == ORIGINAL,
        "expected a {ORIGINAL} original, got {}",
        original.size()
    );
    let channels = original.channels().each_ref().map(|channel| {
        channel
            .chunks(ORIGINAL.width)
            .flat_map(|row| &row[..CROP.width])
            .copied()
            .collect()
    });
    RgbImage::new(CROP, channels)
}
