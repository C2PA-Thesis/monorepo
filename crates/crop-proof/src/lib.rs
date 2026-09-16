//! HyperVerITAS PST proof that a published image is a rectangle of a
//! fingerprinted original.
//!
//! Extracted from HyperVerITAS (<https://github.com/C2PA-Thesis/HyperVerITAS>
//! at 798554f, MIT, see LICENSE). The original is fixed at 1024x512; the
//! rectangle is a public input of the proof. The fingerprint itself lives in
//! the `fingerprint` crate, shared with the capture device.

use std::fmt;

use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};

mod crop;
mod iop;
mod params;
mod wire;

pub use crop::{prove, verify};
pub use fingerprint::{Fingerprint, RgbImage, Size, ALGORITHM as FINGERPRINT_ALGORITHM, ORIGINAL};
pub use params::{setup, setup_with_rng, ProverParams, VerifierParams};

/// Names the parameter set, which the crop statement does not change.
pub const PROTOCOL: &str = "hyperveritas-pst-crop-v1";

/// log2 of the original's pixel count, so the number of variables of every
/// polynomial over the original.
const NUM_VARS: usize = 19;

type F = fingerprint::F;
type Pcs = subroutines::pcs::prelude::MultilinearKzgPCS<ark_bls12_381::Bls12_381>;

/// The region of the original a published image is claimed to be, in pixels
/// from the top-left corner.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rect {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

impl Rect {
    /// What the demo publishes.
    pub const LEFT_HALF: Rect = Rect {
        x: 0,
        y: 0,
        width: 512,
        height: 512,
    };

    pub fn size(self) -> Size {
        Size {
            width: self.width,
            height: self.height,
        }
    }

    pub fn check(self) -> Result<()> {
        ensure!(
            self.width > 0
                && self.height > 0
                && self.x + self.width <= ORIGINAL.width
                && self.y + self.height <= ORIGINAL.height,
            "crop {self} does not fit a {ORIGINAL} original"
        );
        Ok(())
    }

    /// The bytes bound into the proof transcript.
    pub(crate) fn bytes(self) -> Vec<u8> {
        [self.x, self.y, self.width, self.height]
            .iter()
            .flat_map(|value| (*value as u32).to_le_bytes())
            .collect()
    }
}

impl fmt::Display for Rect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}x{} at ({}, {})",
            self.width, self.height, self.x, self.y
        )
    }
}

/// The only edit this crate proves.
pub fn crop(original: &RgbImage, rect: Rect) -> Result<RgbImage> {
    ensure!(
        original.size() == ORIGINAL,
        "expected a {ORIGINAL} original, got {}",
        original.size()
    );
    rect.check()?;
    let channels = original.channels().each_ref().map(|channel| {
        channel
            .chunks(ORIGINAL.width)
            .skip(rect.y)
            .take(rect.height)
            .flat_map(|row| &row[rect.x..rect.x + rect.width])
            .copied()
            .collect()
    });
    RgbImage::new(rect.size(), channels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rects_outside_the_original_or_empty_are_refused() {
        assert!(Rect::LEFT_HALF.check().is_ok());
        for bad in [
            Rect {
                x: 513,
                y: 0,
                width: 512,
                height: 512,
            },
            Rect {
                x: 0,
                y: 1,
                width: 1024,
                height: 512,
            },
            Rect {
                x: 0,
                y: 0,
                width: 0,
                height: 1,
            },
        ] {
            assert!(bad.check().is_err(), "{bad}");
        }
    }

    #[test]
    fn crop_picks_the_rectangle() {
        let channels = std::array::from_fn(|c| {
            (0..ORIGINAL.pixels())
                .map(|i| ((i + c) % 251) as u8)
                .collect::<Vec<u8>>()
        });
        let original = RgbImage::new(ORIGINAL, channels).unwrap();
        let rect = Rect {
            x: 100,
            y: 50,
            width: 3,
            height: 2,
        };
        let cropped = crop(&original, rect).unwrap();
        assert_eq!(cropped.size(), rect.size());
        let at = |x: usize, y: usize| original.channels()[1][y * ORIGINAL.width + x];
        assert_eq!(
            cropped.channels()[1],
            vec![
                at(100, 50),
                at(101, 50),
                at(102, 50),
                at(100, 51),
                at(101, 51),
                at(102, 51)
            ]
        );
    }
}
