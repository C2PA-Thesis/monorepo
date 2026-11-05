//! HyperVerITAS PST proof that a published image is a rectangle of a
//! fingerprinted original, turned by a multiple of 90 degrees.
//!
//! Extracted from HyperVerITAS (<https://github.com/C2PA-Thesis/HyperVerITAS>
//! at 798554f, MIT, see LICENSE). The original has any size up to 2^19
//! pixels, taken from its signed fingerprint; the rectangle is a public input
//! of the proof, and so is the turn. The fingerprint itself lives in the `fingerprint` crate,
//! shared with the capture device.

use std::fmt;

use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};

mod crop;
#[cfg(feature = "attack-hooks")]
pub mod forge;
#[cfg(feature = "attack-hooks")]
pub mod hooks;
mod iop;
#[cfg(feature = "attack-hooks")]
pub mod lattice;
mod params;
mod wire;

pub use crop::{prove, verify};
pub use fingerprint::{
    Fingerprint, RgbImage, Size, ALGORITHM as FINGERPRINT_ALGORITHM, MAX_PIXELS,
};
pub use params::{setup, setup_with_rng, ProverParams, VerifierParams};

/// Names the parameter set, which the crop statement does not change.
pub const PROTOCOL: &str = "hyperveritas-pst-crop-v1";

/// Variables of every polynomial over the original. Smaller originals are
/// padded with zeros, so every size shares one set of parameters.
const NUM_VARS: usize = 19;
const _: () = assert!(1 << NUM_VARS == MAX_PIXELS);

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
    /// What the demo publishes: the left half, rounded down.
    pub fn left_half(original: Size) -> Rect {
        Rect {
            x: 0,
            y: 0,
            width: (original.width / 2).max(1),
            height: original.height,
        }
    }

    pub fn size(self) -> Size {
        Size {
            width: self.width,
            height: self.height,
        }
    }

    pub fn check(self, original: Size) -> Result<()> {
        let fits = |start: usize, length: usize, limit: usize| {
            length > 0 && start.checked_add(length).is_some_and(|end| end <= limit)
        };
        ensure!(
            fits(self.x, self.width, original.width) && fits(self.y, self.height, original.height),
            "crop {self} does not fit a {original} original"
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

/// Quarter turns clockwise applied after the crop, 0 to 3. A turn only
/// reorders pixels, so it is proved by the same selection as the crop.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub struct Turn(u8);

impl Turn {
    pub const NONE: Turn = Turn(0);

    pub fn new(quarters: u8) -> Result<Turn> {
        ensure!(
            quarters < 4,
            "a turn is 0 to 3 quarter turns, not {quarters}"
        );
        Ok(Turn(quarters))
    }

    pub fn quarters(self) -> u8 {
        self.0
    }

    pub fn degrees(self) -> u16 {
        u16::from(self.0) * 90
    }

    /// The size of a `size` image after the turn.
    pub fn size(self, size: Size) -> Size {
        if self.0.is_multiple_of(2) {
            size
        } else {
            Size {
                width: size.height,
                height: size.width,
            }
        }
    }

    /// The pixel of a `size` image that lands on (u, v) after the turn.
    pub(crate) fn source(self, size: Size, u: usize, v: usize) -> (usize, usize) {
        let (w, h) = (size.width, size.height);
        match self.0 {
            0 => (u, v),
            1 => (v, h - 1 - u),
            2 => (w - 1 - u, h - 1 - v),
            _ => (w - 1 - v, u),
        }
    }

    pub fn apply(self, image: &RgbImage) -> Result<RgbImage> {
        let size = image.size();
        let turned = self.size(size);
        let channels = image.channels().each_ref().map(|channel| {
            (0..turned.height)
                .flat_map(|v| (0..turned.width).map(move |u| (u, v)))
                .map(|(u, v)| {
                    let (x, y) = self.source(size, u, v);
                    channel[y * size.width + x]
                })
                .collect()
        });
        RgbImage::new(turned, channels)
    }
}

impl TryFrom<u8> for Turn {
    type Error = anyhow::Error;

    fn try_from(quarters: u8) -> Result<Turn> {
        Turn::new(quarters)
    }
}

impl From<Turn> for u8 {
    fn from(turn: Turn) -> u8 {
        turn.0
    }
}

impl fmt::Display for Turn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} degrees clockwise", self.degrees())
    }
}

/// The edit this crate proves: the `rect` of the original, then `turn`.
pub fn edit(original: &RgbImage, rect: Rect, turn: Turn) -> Result<RgbImage> {
    turn.apply(&crop(original, rect)?)
}

pub fn crop(original: &RgbImage, rect: Rect) -> Result<RgbImage> {
    let size = original.size();
    size.check()?;
    rect.check(size)?;
    let channels = original.channels().each_ref().map(|channel| {
        channel
            .chunks(size.width)
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

    const ORIGINAL: Size = fingerprint::TEST_SIZE;

    #[test]
    fn rects_outside_the_original_or_empty_are_refused() {
        assert!(Rect::left_half(ORIGINAL).check(ORIGINAL).is_ok());
        let four_by_three = Size {
            width: 836,
            height: 627,
        };
        assert_eq!(
            Rect::left_half(four_by_three),
            Rect {
                x: 0,
                y: 0,
                width: 418,
                height: 627
            }
        );
        assert!(Rect {
            x: 0,
            y: 0,
            width: 836,
            height: 627
        }
        .check(four_by_three)
        .is_ok());
        for bad in [
            Rect {
                x: 0,
                y: 0,
                width: 837,
                height: 1,
            },
            Rect {
                x: usize::MAX,
                y: 0,
                width: 2,
                height: 1,
            },
        ] {
            assert!(bad.check(four_by_three).is_err(), "{bad}");
        }
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
            assert!(bad.check(ORIGINAL).is_err(), "{bad}");
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

    #[test]
    fn turns_rotate_clockwise() {
        // 3x2, values 0..6 in reading order:
        // 0 1 2
        // 3 4 5
        let size = Size {
            width: 3,
            height: 2,
        };
        let image = RgbImage::new(size, std::array::from_fn(|_| (0..6).collect())).unwrap();
        let expected: [(Size, [u8; 6]); 4] = [
            (size, [0, 1, 2, 3, 4, 5]),
            (
                Size {
                    width: 2,
                    height: 3,
                },
                [3, 0, 4, 1, 5, 2],
            ),
            (size, [5, 4, 3, 2, 1, 0]),
            (
                Size {
                    width: 2,
                    height: 3,
                },
                [2, 5, 1, 4, 0, 3],
            ),
        ];
        for (quarters, (turned_size, pixels)) in expected.into_iter().enumerate() {
            let turn = Turn::new(quarters as u8).unwrap();
            let turned = turn.apply(&image).unwrap();
            assert_eq!(turned.size(), turned_size, "{turn}");
            assert_eq!(turned.channels()[0], pixels.to_vec(), "{turn}");
        }
    }

    #[test]
    fn turns_past_three_quarters_are_refused() {
        assert!(Turn::new(4).is_err());
        assert!(serde_json::from_str::<Turn>("4").is_err());
        assert_eq!(
            serde_json::from_str::<Turn>("3").unwrap(),
            Turn::new(3).unwrap()
        );
        assert_eq!(serde_json::to_string(&Turn::new(1).unwrap()).unwrap(), "1");
    }
}
