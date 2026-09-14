use std::{
    fs::{self, File, OpenOptions},
    io::{BufReader, Write},
    os::unix::fs::OpenOptionsExt,
    path::Path,
};

use anyhow::{anyhow, ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use crop_proof::{Fingerprint, RgbImage, Size, ORIGINAL};
use image::{imageops::FilterType, ImageFormat};
use p256::{
    ecdsa::{
        signature::{Signer, Verifier},
        Signature, SigningKey, VerifyingKey,
    },
    pkcs8::{DecodePrivateKey, DecodePublicKey, EncodePrivateKey, EncodePublicKey, LineEnding},
};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

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
    pub fn new(latitude: f64, longitude: f64) -> Self {
        let mut salt = [0u8; 32];
        // A zero first byte keeps the salt below the BN254 modulus.
        OsRng.fill_bytes(&mut salt[1..]);
        Self {
            latitude,
            longitude,
            salt: format!("0x{}", hex::encode(salt)),
        }
    }
}

/// The device's signature over both public values. It is the only thing
/// binding the crop proof and the location proof to the same capture.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub fingerprint: Fingerprint,
    /// MiMC commitment to the coordinate and salt, as a BN254 scalar.
    pub envelope: String,
    /// Asserted by the device, not by a trusted clock.
    pub captured_at: String,
    /// SHA-256 of the device public key in SubjectPublicKeyInfo DER.
    pub device: String,
    /// Base64 DER ECDSA P-256 signature over the compact JSON of the fields above.
    pub signature: String,
}

#[derive(Serialize)]
struct SignedFields<'a> {
    fingerprint: &'a Fingerprint,
    envelope: &'a str,
    captured_at: &'a str,
    device: &'a str,
}

impl Receipt {
    pub fn sign(fingerprint: Fingerprint, envelope: String, device: &DeviceKey) -> Result<Self> {
        let captured_at = OffsetDateTime::now_utc()
            .replace_nanosecond(0)?
            .format(&Rfc3339)?;
        let mut receipt = Self {
            fingerprint,
            envelope,
            captured_at,
            device: device.id()?,
            signature: String::new(),
        };
        receipt.resign(device)?;
        Ok(receipt)
    }

    /// Signs the current fields, which may have been edited since.
    pub fn resign(&mut self, device: &DeviceKey) -> Result<()> {
        let signature: Signature = device.0.sign(&self.signed_bytes()?);
        self.signature = BASE64.encode(signature.to_der());
        Ok(())
    }

    pub fn verify(&self, trusted: &VerifyingKey) -> Result<()> {
        let trusted_id = key_id(trusted)?;
        ensure!(
            self.device == trusted_id,
            "signed by device {}, not the trusted device {}",
            short(&self.device),
            short(&trusted_id)
        );
        let der = BASE64
            .decode(&self.signature)
            .context("the signature is not base64")?;
        let signature =
            Signature::from_der(&der).map_err(|_| anyhow!("the signature is not DER ECDSA"))?;
        trusted
            .verify(&self.signed_bytes()?, &signature)
            .map_err(|_| anyhow!("the device signature does not match the receipt"))
    }

    fn signed_bytes(&self) -> Result<Vec<u8>> {
        Ok(serde_json::to_vec(&SignedFields {
            fingerprint: &self.fingerprint,
            envelope: &self.envelope,
            captured_at: &self.captured_at,
            device: &self.device,
        })?)
    }
}

/// Stand-in for the camera's hardware key.
pub struct DeviceKey(SigningKey);

impl DeviceKey {
    pub fn generate() -> Self {
        Self(SigningKey::random(&mut OsRng))
    }

    pub fn load(path: &Path) -> Result<Self> {
        let pem =
            fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let key = SigningKey::from_pkcs8_pem(&pem)
            .map_err(|_| anyhow!("{} is not a P-256 private key", path.display()))?;
        Ok(Self(key))
    }

    pub fn save(&self, private: &Path, public: &Path) -> Result<()> {
        let private_pem = self
            .0
            .to_pkcs8_pem(LineEnding::LF)
            .map_err(|error| anyhow!("{error}"))?;
        write_private(private, private_pem.as_bytes())?;
        let public_pem = self
            .public()
            .to_public_key_pem(LineEnding::LF)
            .map_err(|error| anyhow!("{error}"))?;
        Ok(fs::write(public, public_pem)?)
    }

    pub fn public(&self) -> VerifyingKey {
        *self.0.verifying_key()
    }

    pub fn id(&self) -> Result<String> {
        key_id(&self.public())
    }
}

pub fn load_public_key(path: &Path) -> Result<VerifyingKey> {
    let pem = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    VerifyingKey::from_public_key_pem(&pem)
        .map_err(|_| anyhow!("{} is not a P-256 public key", path.display()))
}

pub fn key_id(key: &VerifyingKey) -> Result<String> {
    let der = key
        .to_public_key_der()
        .map_err(|error| anyhow!("{error}"))?;
    Ok(hex::encode(Sha256::digest(der.as_bytes())))
}

/// First and last four hex digits, enough to tell keys apart on screen.
pub fn short(id: &str) -> String {
    match id.len() {
        0..=8 => id.to_string(),
        len => format!("{}…{}", &id[..4], &id[len - 4..]),
    }
}

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

pub fn write_png(image: &RgbImage, path: &Path) -> Result<()> {
    let [r, g, b] = image.channels();
    let interleaved = r
        .iter()
        .zip(g)
        .zip(b)
        .flat_map(|((r, g), b)| [*r, *g, *b])
        .collect();
    let size = image.size();
    image::RgbImage::from_raw(size.width as u32, size.height as u32, interleaved)
        .context("pixel buffer does not match the image size")?
        .save_with_format(path, ImageFormat::Png)
        .with_context(|| format!("writing {}", path.display()))
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
