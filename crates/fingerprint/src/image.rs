use std::fmt;

use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};

/// The only image size the fingerprint, and so the crop proof, is defined for.
pub const ORIGINAL: Size = Size {
    width: 1024,
    height: 512,
};

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
}

/// A deterministic original, so every implementation of the fingerprint can
/// be checked against one digest. The page has the same formula.
#[cfg(any(test, target_arch = "wasm32"))]
pub(crate) fn test_image() -> RgbImage {
    let channels = std::array::from_fn(|channel| {
        (0..ORIGINAL.pixels())
            .map(|i| ((i * 7 + channel * 13 + (i >> 9)) & 0xff) as u8)
            .collect()
    });
    RgbImage::new(ORIGINAL, channels).expect("channels have the right length")
}
