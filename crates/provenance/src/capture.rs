//! A capture: the private original, the location witness and the signed
//! receipt, as one directory.
//!
//! ```text
//! original.png         the 1024x512 original, private
//! secrets.json         coordinate and salt, private, mode 0600, when the laptop proves the location
//! location_proof.json  the proof the phone made instead, public; then the laptop never sees the coordinate
//! receipt.json         the device-signed receipt, public
//! capture.json         the cell and crop chosen at capture, and how the capture was made
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
    location::{check_resolution, LocationProof, LocationTool},
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
    /// Float32 bit patterns of the trigonometric hints the envelope also
    /// commits to. location-proof computes them from the coordinate when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hints: Option<[u32; 8]>,
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
            hints: None,
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
    /// Where the app's location fix came from. Recorded, not signed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<LocationSource>,
}

/// What the phone's location service said about its fix.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocationSource {
    /// The fix came from a simulator or a GPX file, not from radios.
    pub simulated: bool,
    /// The fix came from an external accessory, such as a GPS receiver.
    pub accessory: bool,
}

/// How the location proof gets made.
#[derive(Clone, Debug)]
pub enum Witness {
    /// The laptop holds the coordinate and proves when publishing.
    Secrets(Secrets),
    /// The phone proved and sent only the proof.
    Proof(LocationProof),
}

pub struct Capture {
    pub original: RgbImage,
    pub witness: Witness,
    pub receipt: Receipt,
    pub summary: Summary,
}

const ORIGINAL: &str = "original.png";
const SECRETS: &str = "secrets.json";
const LOCATION_PROOF: &str = "location_proof.json";
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
        check_resolution(region.resolution)?;
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
            witness: Witness::Secrets(secrets),
            receipt,
            summary: Summary {
                source: Source::Simulated,
                cell: region.cell,
                resolution: region.resolution,
                crop,
                accuracy_meters: None,
                location: None,
            },
        })
    }

    /// The coordinate and salt, when the laptop holds them.
    pub fn secrets(&self) -> Option<&Secrets> {
        match &self.witness {
            Witness::Secrets(secrets) => Some(secrets),
            Witness::Proof(_) => None,
        }
    }

    pub fn write(&self, dir: &Path) -> Result<()> {
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        image::write_png(&self.original, &dir.join(ORIGINAL))?;
        match &self.witness {
            Witness::Secrets(secrets) => {
                write_private(&dir.join(SECRETS), &serde_json::to_vec_pretty(secrets)?)?
            }
            Witness::Proof(proof) => {
                fs::write(dir.join(LOCATION_PROOF), serde_json::to_vec_pretty(proof)?)?
            }
        }
        fs::write(dir.join(RECEIPT), serde_json::to_vec_pretty(&self.receipt)?)?;
        fs::write(dir.join(SUMMARY), serde_json::to_vec_pretty(&self.summary)?)?;
        Ok(())
    }

    /// The public part of a capture directory, without decoding the original.
    pub fn read_public(dir: &Path) -> Result<(Receipt, Summary)> {
        Ok((
            read_json(&dir.join(RECEIPT))?,
            read_json(&dir.join(SUMMARY))?,
        ))
    }

    pub fn read(dir: &Path) -> Result<Self> {
        let witness = match dir.join(SECRETS).exists() {
            true => Witness::Secrets(read_json(&dir.join(SECRETS))?),
            false => Witness::Proof(read_json(&dir.join(LOCATION_PROOF))?),
        };
        Ok(Self {
            original: image::read_png(&dir.join(ORIGINAL))?,
            witness,
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
