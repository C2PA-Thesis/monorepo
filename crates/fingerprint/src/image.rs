use std::fmt;

use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};

/// The most pixels an original may have. The crop proof's polynomials have
/// 19 variables, so a smaller original is padded with zeros to this length.
pub const MAX_PIXELS: usize = 1 << 19;

/// The size of the test image, and of every original before full-frame captures.
pub const TEST_SIZE: Size = Size {
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

    /// Whether an original of this size can be fingerprinted and proved.
    pub fn check(self) -> Result<()> {
        ensure!(
            self.width > 0
                && self.height > 0
                && self
                    .width
                    .checked_mul(self.height)
                    .is_some_and(|pixels| pixels <= MAX_PIXELS),
            "an original must have between 1 and {MAX_PIXELS} pixels, got {self}"
        );
        Ok(())
    }

    /// The size an original of a `width` x `height` photo gets: the photo
    /// itself if it fits in `MAX_PIXELS`, otherwise the largest size with
    /// the same aspect ratio that does. The app and the capture page compute
    /// the same thing with the same float operations.
    pub fn fit(width: usize, height: usize) -> Size {
        let photo = Size { width, height };
        if photo.check().is_ok() {
            return photo;
        }
        let scale = (MAX_PIXELS as f64 / (width as f64 * height as f64)).sqrt();
        let mut fitted = Size {
            width: ((width as f64 * scale).floor() as usize).max(1),
            height: ((height as f64 * scale).floor() as usize).max(1),
        };
        // Flooring both sides keeps the product under the limit; this only
        // guards against a photo far thinner than any camera makes.
        while fitted.pixels() > MAX_PIXELS {
            if fitted.width >= fitted.height {
                fitted.width -= 1;
            } else {
                fitted.height -= 1;
            }
        }
        fitted
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

/// Channel `channel` of the deterministic test image, `pixels` values long.
/// The page, the C ABI test and the app use the same formula, so every
/// implementation of the fingerprint can be checked against a pinned digest.
pub fn test_channel(channel: usize, pixels: usize) -> Vec<u8> {
    (0..pixels)
        .map(|i| ((i * 7 + channel * 13 + (i >> 9)) & 0xff) as u8)
        .collect()
}

/// The deterministic test image of a given size.
pub fn test_image(size: Size) -> RgbImage {
    let channels = std::array::from_fn(|channel| test_channel(channel, size.pixels()));
    RgbImage::new(size, channels).expect("channels have the right length")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_over_the_limit_or_empty_are_refused() {
        assert!(TEST_SIZE.check().is_ok());
        assert!(Size {
            width: 836,
            height: 627
        }
        .check()
        .is_ok());
        for bad in [
            Size {
                width: 0,
                height: 10,
            },
            Size {
                width: 1025,
                height: 512,
            },
            Size {
                width: usize::MAX,
                height: 2,
            },
        ] {
            assert!(bad.check().is_err(), "{bad}");
        }
    }

    #[test]
    fn fit_keeps_the_aspect_ratio_under_the_limit() {
        // An iPhone 12 MP photo, both ways up.
        assert_eq!(
            Size::fit(4032, 3024),
            Size {
                width: 836,
                height: 627
            }
        );
        assert_eq!(
            Size::fit(3024, 4032),
            Size {
                width: 627,
                height: 836
            }
        );
        // 48 MP and 16:9.
        assert_eq!(
            Size::fit(8064, 6048),
            Size {
                width: 836,
                height: 627
            }
        );
        assert_eq!(
            Size::fit(1920, 1080),
            Size {
                width: 965,
                height: 543
            }
        );
        // Already small enough: unchanged.
        assert_eq!(Size::fit(1024, 512), TEST_SIZE);
        assert_eq!(
            Size::fit(640, 480),
            Size {
                width: 640,
                height: 480
            }
        );
        for (width, height) in [(4032, 3024), (100_000, 1), (1, 100_000), (5000, 4999)] {
            let fitted = Size::fit(width, height);
            assert!(fitted.check().is_ok(), "{width}x{height} -> {fitted}");
        }
    }
}
