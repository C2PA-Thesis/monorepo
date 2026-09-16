//! The device receipt, the device key that signs it, and the keys a reader trusts.

use std::{fs, path::Path};

use anyhow::{anyhow, ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use crop_proof::Fingerprint;
use p256::{
    ecdsa::{
        signature::{Signer, Verifier},
        Signature, SigningKey, VerifyingKey,
    },
    pkcs8::{DecodePrivateKey, DecodePublicKey, EncodePrivateKey, EncodePublicKey, LineEnding},
};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::capture::write_private;

/// The device's signature over both public values. It is the only thing
/// binding the crop proof and the location proof to the same capture.
///
/// The signed bytes are the compact JSON of the first four fields, in this
/// order, with the fingerprint's own field order. A device that builds the
/// receipt elsewhere, like the capture page, must serialize it identically.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub fingerprint: Fingerprint,
    /// MiMC commitment to the coordinate and salt, as a BN254 scalar.
    pub envelope: String,
    /// RFC 3339, asserted by the device, not by a trusted clock.
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
    pub fn sign(
        fingerprint: Fingerprint,
        envelope: String,
        captured_at: String,
        device: &DeviceKey,
    ) -> Result<Self> {
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

    pub fn verify(&self, trusted: &TrustedKeys) -> Result<()> {
        let key = trusted
            .find(&self.device)
            .ok_or_else(|| anyhow!("device {} is not trusted", short(&self.device)))?;
        let der = BASE64
            .decode(&self.signature)
            .context("the signature is not base64")?;
        let signature =
            Signature::from_der(&der).map_err(|_| anyhow!("the signature is not DER ECDSA"))?;
        key.verify(&self.signed_bytes()?, &signature)
            .map_err(|_| anyhow!("the device signature does not match the receipt"))
    }

    /// SHA-256 of the fingerprint's JSON. It only names the fingerprint on
    /// screen and in file names; the crop proof checks the fingerprint itself.
    pub fn fingerprint_digest(&self) -> String {
        hex::encode(Sha256::digest(
            serde_json::to_vec(&self.fingerprint).expect("a fingerprint serializes to JSON"),
        ))
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

/// Stand-in for the camera's hardware key. The demo signs with one of these
/// on the laptop; the capture page keeps its own in the browser.
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

    pub fn save(&self, path: &Path) -> Result<()> {
        let pem = self
            .0
            .to_pkcs8_pem(LineEnding::LF)
            .map_err(|error| anyhow!("{error}"))?;
        write_private(path, pem.as_bytes())
    }

    pub fn public(&self) -> VerifyingKey {
        *self.0.verifying_key()
    }

    pub fn id(&self) -> Result<String> {
        key_id(&self.public())
    }
}

/// The device public keys a reader accepts: one PEM file per key in a
/// directory, named by key id. Setup adds the laptop's simulated device and
/// pairing adds the phone's.
pub struct TrustedKeys(Vec<(String, VerifyingKey)>);

impl TrustedKeys {
    pub fn load(dir: &Path) -> Result<Self> {
        let entries = fs::read_dir(dir)
            .with_context(|| format!("reading {}; run `provenance setup` first", dir.display()))?;
        let mut keys = Vec::new();
        for entry in entries {
            let path = entry?.path();
            if path.extension().is_some_and(|ext| ext == "pem") {
                let key = parse_public_key(&fs::read_to_string(&path)?)
                    .with_context(|| format!("reading {}", path.display()))?;
                keys.push((key_id(&key)?, key));
            }
        }
        ensure!(!keys.is_empty(), "{} holds no trusted key", dir.display());
        Ok(Self(keys))
    }

    /// Writes `key` into `dir` and returns its id.
    pub fn add(dir: &Path, key: &VerifyingKey) -> Result<String> {
        let id = key_id(key)?;
        let pem = key
            .to_public_key_pem(LineEnding::LF)
            .map_err(|error| anyhow!("{error}"))?;
        fs::create_dir_all(dir)?;
        fs::write(dir.join(format!("{id}.pem")), pem)?;
        Ok(id)
    }

    pub fn find(&self, id: &str) -> Option<&VerifyingKey> {
        self.0
            .iter()
            .find(|(known, _)| known == id)
            .map(|(_, key)| key)
    }
}

pub fn parse_public_key(pem: &str) -> Result<VerifyingKey> {
    VerifyingKey::from_public_key_pem(pem).map_err(|_| anyhow!("not a P-256 public key in PEM"))
}

pub fn key_id(key: &VerifyingKey) -> Result<String> {
    let der = key
        .to_public_key_der()
        .map_err(|error| anyhow!("{error}"))?;
    Ok(hex::encode(Sha256::digest(der.as_bytes())))
}

/// First and last four hex digits, enough to tell keys apart on screen.
pub fn short(id: &str) -> String {
    let (prefix, digits) = id.split_at(if id.starts_with("0x") { 2 } else { 0 });
    match digits.len() {
        0..=8 => id.to_string(),
        len => format!("{prefix}{}…{}", &digits[..4], &digits[len - 4..]),
    }
}
