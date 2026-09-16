//! Reading photos into the crop proof's pixel layout and writing PNGs back.

use std::{
    fs::File,
    io::{BufReader, Cursor},
    path::Path,
};

use anyhow::{Context, Result};
use crop_proof::{RgbImage, Size, ORIGINAL};
use image::{imageops::FilterType, ImageFormat};

/// The photo as the crop proof takes it: RGB, resized to exactly 1024x512.
pub fn load_photo(path: &Path) -> Result<RgbImage> {
    let photo = image::open(path)
        .with_context(|| format!("reading {}", path.display()))?
        .to_rgb8();
    let resized = image::imageops::resize(
        &photo,
        ORIGINAL.width as u32,
        ORIGINAL.height as u32,
        FilterType::Lanczos3,
    );
    to_rgb(&resized)
}

pub fn read_png(path: &Path) -> Result<RgbImage> {
    let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let decoded = image::load(BufReader::new(file), ImageFormat::Png)
        .with_context(|| format!("{} is not a PNG", path.display()))?;
    to_rgb(&decoded.to_rgb8())
}

pub fn decode_png(bytes: &[u8]) -> Result<RgbImage> {
    let decoded =
        image::load_from_memory_with_format(bytes, ImageFormat::Png).context("not a PNG")?;
    to_rgb(&decoded.to_rgb8())
}

pub fn write_png(image: &RgbImage, path: &Path) -> Result<()> {
    std::fs::write(path, encode_png(image)?).with_context(|| format!("writing {}", path.display()))
}

pub fn encode_png(image: &RgbImage) -> Result<Vec<u8>> {
    let [r, g, b] = image.channels();
    let interleaved = r
        .iter()
        .zip(g)
        .zip(b)
        .flat_map(|((r, g), b)| [*r, *g, *b])
        .collect();
    let size = image.size();
    let mut png = Cursor::new(Vec::new());
    image::RgbImage::from_raw(size.width as u32, size.height as u32, interleaved)
        .context("pixel buffer does not match the image size")?
        .write_to(&mut png, ImageFormat::Png)
        .context("encoding a PNG")?;
    Ok(png.into_inner())
}

fn to_rgb(image: &image::RgbImage) -> Result<RgbImage> {
    let size = Size {
        width: image.width() as usize,
        height: image.height() as usize,
    };
    let mut channels: [Vec<u8>; 3] = Default::default();
    for pixel in image.pixels() {
        for (channel, value) in channels.iter_mut().zip(pixel.0) {
            channel.push(value);
        }
    }
    RgbImage::new(size, channels)
}
