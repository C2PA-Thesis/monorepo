//! HyperVerITAS PST proof that a published image is the left half of a
//! fingerprinted original.
//!
//! Extracted from HyperVerITAS (<https://github.com/C2PA-Thesis/HyperVerITAS>
//! at 798554f, MIT, see LICENSE). The statement is fixed to a 1024x512
//! original and its 512x512 left half, which is what the thesis PoC publishes.

use std::fmt;

use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};

mod crop;
mod fingerprint;
mod iop;
mod params;
mod wire;

pub use crop::{prove, verify};
pub use fingerprint::{Fingerprint, FINGERPRINT_ALGORITHM};
pub use params::{setup, setup_with_rng, ProverParams, VerifierParams};

pub const PROTOCOL: &str = "hyperveritas-pst-crop-v1";
pub const ORIGINAL: Size = Size {
    width: 1024,
    height: 512,
};
pub const CROP: Size = Size {
    width: 512,
    height: 512,
};

/// log2 of the original's pixel count, so the number of variables of every
/// polynomial over the original.
const NUM_VARS: usize = 19;

type F = ark_bls12_381::Fr;
type Pcs = subroutines::pcs::prelude::MultilinearKzgPCS<ark_bls12_381::Bls12_381>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Size {
    pub width: usize,
    pub height: usize,
}

impl Size {
    pub const fn pixels(self) -> usize {
        self.width * self.height
    }
}

impl fmt::Display for Size {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}x{}", self.width, self.height)
    }
}

/// RGB pixels in row-major order: the pixel at (x, y) is index `y * width + x`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RgbImage {
    size: Size,
    channels: [Vec<u8>; 3],
}

impl RgbImage {
    pub fn new(size: Size, channels: [Vec<u8>; 3]) -> Result<Self> {
        for (name, channel) in ["R", "G", "B"].iter().zip(&channels) {
            ensure!(
                channel.len() == size.pixels(),
                "channel {name} has {} values, a {size} image needs {}",
                channel.len(),
                size.pixels()
            );
        }
        Ok(Self { size, channels })
    }

    pub fn size(&self) -> Size {
        self.size
    }

    pub fn channels(&self) -> &[Vec<u8>; 3] {
        &self.channels
    }

    /// The only edit this crate proves.
    pub fn left_half(&self) -> Result<Self> {
        ensure!(
            self.size == ORIGINAL,
            "expected a {ORIGINAL} original, got {}",
            self.size
        );
        let channels = self.channels.each_ref().map(|channel| {
            channel
                .chunks(ORIGINAL.width)
                .flat_map(|row| &row[..CROP.width])
                .copied()
                .collect()
        });
        Self::new(CROP, channels)
    }
}
