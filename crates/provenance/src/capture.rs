//! A capture: the private witness and the signed receipt, as one directory.
//!
//! ```text
//! original.png   the 1024x512 original, private
//! secrets.json   coordinate and salt, private, mode 0600
//! receipt.json   the device-signed receipt, public
//! capture.json   the cell and crop chosen at capture, and how the capture was made
//! ```

use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::Path,
};

use anyhow::{Context, Result};
use crop_proof::{Fingerprint, Rect, RgbImage};
use rand::{rngs::OsRng, RngCore};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

use crate::{
    image,
    location::LocationTool,
    receipt::{DeviceKey, Receipt},
};

/// The coordinate as the location prover takes it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Coordinate {
    pub latitude: f64,
    pub longitude: f64,
}

/// What only the photographer holds: the coordinate and the salt hiding it.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Secrets {
    pub latitude: f64,
    pub longitude: f64,
    pub salt: String,
}

impl Secrets {
    /// Draws a fresh salt. Without one, the envelope could be brute-forced
    /// over plausible coordinates.
    pub fn new(coordinate: Coordinate) -> Self {
        let mut salt = [0u8; 32];
        // A zero first byte keeps the salt below the BN254 modulus.
        OsRng.fill_bytes(&mut salt[1..]);
        Self {
            latitude: coordinate.latitude,
            longitude: coordinate.longitude,
            salt: format!("0x{}", hex::encode(salt)),
        }
    }

    pub fn coordinate(&self) -> Coordinate {
        Coordinate {
            latitude: self.latitude,
            longitude: self.longitude,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Source {
    /// The laptop's own device key signed a photo at a chosen coordinate.
    Simulated,
    /// A paired phone signed its camera frame at its GPS fix.
    Phone,
}

/// The public choices made at capture, kept next to the receipt.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Summary {
    pub source: Source,
    /// H3 cell the location proof will claim.
    pub cell: String,
    pub resolution: u8,
    /// The rectangle of the original that gets published.
    pub crop: Rect,
    /// Horizontal accuracy the device reported for its fix. Recorded, not signed.
    pub accuracy_meters: Option<f64>,
}

pub struct Capture {
    pub original: RgbImage,
    pub secrets: Secrets,
    pub receipt: Receipt,
    pub summary: Summary,
}

const ORIGINAL: &str = "original.png";
const SECRETS: &str = "secrets.json";
const RECEIPT: &str = "receipt.json";
const SUMMARY: &str = "capture.json";

impl Capture {
    /// A capture signed by the laptop's own device key, for the demo and the attacks.
    pub fn simulate(
        tool: &LocationTool,
        device: &DeviceKey,
        original: RgbImage,
        coordinate: Coordinate,
        cell: &str,
        crop: Rect,
    ) -> Result<Self> {
        let region = tool.region(cell)?;
        crop.check()?;
        let secrets = Secrets::new(coordinate);
        let receipt = Receipt::sign(
            Fingerprint::of(&original)?,
            tool.commit(&secrets)?,
            now()?,
            device,
        )?;
        Ok(Self {
            original,
            secrets,
            receipt,
            summary: Summary {
                source: Source::Simulated,
                cell: region.cell,
                resolution: region.resolution,
                crop,
                accuracy_meters: None,
            },
        })
    }

    pub fn write(&self, dir: &Path) -> Result<()> {
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        image::write_png(&self.original, &dir.join(ORIGINAL))?;
        write_private(
            &dir.join(SECRETS),
            &serde_json::to_vec_pretty(&self.secrets)?,
        )?;
        fs::write(dir.join(RECEIPT), serde_json::to_vec_pretty(&self.receipt)?)?;
        fs::write(dir.join(SUMMARY), serde_json::to_vec_pretty(&self.summary)?)?;
        Ok(())
    }

    pub fn read(dir: &Path) -> Result<Self> {
        Ok(Self {
            original: image::read_png(&dir.join(ORIGINAL))?,
            secrets: read_json(&dir.join(SECRETS))?,
            receipt: read_json(&dir.join(RECEIPT))?,
            summary: read_json(&dir.join(SUMMARY))?,
        })
    }
}

/// The current time as a receipt carries it: RFC 3339, UTC, whole seconds.
pub fn now() -> Result<String> {
    Ok(OffsetDateTime::now_utc()
        .replace_nanosecond(0)?
        .format(&Rfc3339)?)
}

pub fn write_private(path: &Path, contents: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(file.write_all(contents)?)
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("parsing {}", path.display()))
}
