//! The paired devices. Each trusted key is a PEM in `trusted/`, named by key
//! id, and may have a `<id>.json` next to it saying what kind of key it is.
//! Readers only use the PEMs; the sidecar is for the presenter and `devices`.

use std::{fs, path::PathBuf};

use anyhow::{bail, ensure, Context, Result};
use p256::ecdsa::VerifyingKey;
use serde::{Deserialize, Serialize};

use crate::receipt::{key_id, TrustedKeys};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeviceKind {
    /// The app, which says its key is in the phone's Secure Enclave. Shown as
    /// declared unless the sidecar carries a verified App Attest attestation.
    SecureEnclave,
    /// The app, with a key in the Keychain (the simulator has no Secure Enclave).
    Software,
    /// The capture page, with a key in the browser.
    Browser,
    /// The laptop's own key, which signs the demo's simulated captures.
    Laptop,
}

impl DeviceKind {
    /// The kind as `provenance devices` and the presenter page show it, with
    /// the hardware claim marked as the app's own.
    pub fn shown(self) -> &'static str {
        match self {
            DeviceKind::SecureEnclave => "declared: secure-enclave",
            other => other.title(),
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            DeviceKind::SecureEnclave => "secure-enclave",
            DeviceKind::Software => "software",
            DeviceKind::Browser => "browser",
            DeviceKind::Laptop => "laptop",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceInfo {
    pub kind: DeviceKind,
    pub name: String,
    /// RFC 3339, the laptop's clock.
    pub paired_at: String,
    /// Set when the pairing carried an App Attest attestation that verified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attestation: Option<Attested>,
}

impl DeviceInfo {
    /// The kind as `provenance devices` shows it.
    pub fn shown(&self) -> String {
        match &self.attestation {
            Some(attested) => format!("attested: {} ({})", self.kind.title(), attested.environment),
            None => self.kind.shown().to_string(),
        }
    }
}

/// What the laptop checked of an App Attest attestation when the key paired.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attested {
    /// `development` or `production`, from the aaguid.
    pub environment: String,
    /// Team ID and bundle id that the RP ID matched.
    pub app_id: String,
    /// The App Attest key id, base64, as `generateKey` returns it.
    pub key_id: String,
    pub validation_category: Option<u32>,
    pub bundle_version: Option<String>,
    /// RFC 3339, the laptop's clock.
    pub verified_at: String,
    /// Apple's count of attested keys on the device in 30 days, when the key
    /// registered on a server with a DeviceCheck key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risk_metric: Option<u32>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Device {
    pub id: String,
    /// Missing for keys trusted before sidecars existed.
    #[serde(flatten)]
    pub info: Option<DeviceInfo>,
}

pub struct Devices {
    dir: PathBuf,
}

impl Devices {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// Trusts `key` and records what it is. Returns the key id.
    pub fn add(&self, key: &VerifyingKey, info: &DeviceInfo) -> Result<String> {
        let id = TrustedKeys::add(&self.dir, key)?;
        fs::write(self.sidecar(&id), serde_json::to_vec_pretty(info)?)?;
        Ok(id)
    }

    /// Trusts `key` and records what it is, unless it is trusted already: a
    /// pairing that proves no possession of the key must not relabel it.
    pub fn add_new(&self, key: &VerifyingKey, info: &DeviceInfo) -> Result<String> {
        let id = key_id(key)?;
        if self.is_trusted(&id) {
            return Ok(id);
        }
        self.add(key, info)
    }

    pub fn is_trusted(&self, id: &str) -> bool {
        is_id(id) && self.dir.join(format!("{id}.pem")).exists()
    }

    pub fn info(&self, id: &str) -> Option<DeviceInfo> {
        if !is_id(id) {
            return None;
        }
        let bytes = fs::read(self.sidecar(id)).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// Every trusted key, oldest pairing first, keys without a sidecar last.
    pub fn list(&self) -> Result<Vec<Device>> {
        let entries = fs::read_dir(&self.dir).with_context(|| {
            format!(
                "reading {}; run `provenance setup` first",
                self.dir.display()
            )
        })?;
        let mut devices = Vec::new();
        for entry in entries {
            let path = entry?.path();
            if path.extension().is_none_or(|ext| ext != "pem") {
                continue;
            }
            let Some(id) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            devices.push(Device {
                id: id.to_string(),
                info: self.info(id),
            });
        }
        devices.sort_by(|a, b| {
            let key = |device: &Device| {
                (
                    device.info.is_none(),
                    device
                        .info
                        .as_ref()
                        .map(|info| info.paired_at.clone())
                        .unwrap_or_default(),
                    device.id.clone(),
                )
            };
            key(a).cmp(&key(b))
        });
        Ok(devices)
    }

    /// Removes the one key whose id starts with `prefix`.
    pub fn revoke(&self, prefix: &str) -> Result<Device> {
        ensure!(
            prefix.len() >= 4 && is_id(prefix),
            "give at least four hex digits of the device id"
        );
        let prefix = prefix.to_ascii_lowercase();
        let mut matches: Vec<Device> = self
            .list()?
            .into_iter()
            .filter(|device| device.id.starts_with(&prefix))
            .collect();
        let device = match matches.len() {
            0 => bail!("no trusted device id starts with {prefix}"),
            1 => matches.remove(0),
            count => bail!("{count} trusted devices start with {prefix}; give more digits"),
        };
        fs::remove_file(self.dir.join(format!("{}.pem", device.id)))?;
        match fs::remove_file(self.sidecar(&device.id)) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error.into()),
            _ => {}
        }
        Ok(device)
    }

    fn sidecar(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }
}

fn is_id(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::receipt::DeviceKey;

    fn info(kind: DeviceKind, paired_at: &str) -> DeviceInfo {
        DeviceInfo {
            kind,
            name: "test".to_string(),
            paired_at: paired_at.to_string(),
            attestation: None,
        }
    }

    #[test]
    fn lists_and_revokes_by_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let devices = Devices::new(dir.path().to_path_buf());
        let old = DeviceKey::generate();
        let phone = DeviceKey::generate();
        TrustedKeys::add(dir.path(), &old.public()).unwrap();
        let id = devices
            .add(
                &phone.public(),
                &info(DeviceKind::SecureEnclave, "2026-10-30T12:00:00Z"),
            )
            .unwrap();
        assert_eq!(id, phone.id().unwrap());

        let listed = devices.list().unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].id, id);
        assert_eq!(
            listed[0].info.as_ref().unwrap().kind,
            DeviceKind::SecureEnclave
        );
        assert!(listed[1].info.is_none());
        assert_eq!(
            serde_json::to_value(&listed[0]).unwrap()["kind"],
            "secure-enclave"
        );

        assert!(devices.revoke("zz").is_err());
        assert!(devices.revoke(&id[..8]).is_ok());
        assert!(!devices.is_trusted(&id));
        assert!(!dir.path().join(format!("{id}.json")).exists());
        assert!(devices.revoke(&id[..8]).is_err());
        assert!(devices.is_trusted(&old.id().unwrap()));
    }

    #[test]
    fn shows_an_attested_key_and_reads_old_sidecars() {
        let mut attested = info(DeviceKind::SecureEnclave, "2026-11-05T12:00:00Z");
        assert_eq!(attested.shown(), "declared: secure-enclave");
        attested.attestation = Some(Attested {
            environment: "development".to_string(),
            app_id: "ABCDE12345.io.github.c2pa-thesis.capture".to_string(),
            key_id: "zgSY9YSD+7TaDXssY6WlOPVS1K3Lmk+pFhlcSWE+ZV0=".to_string(),
            validation_category: Some(3),
            bundle_version: Some("1".to_string()),
            verified_at: "2026-11-05T12:00:00Z".to_string(),
            risk_metric: Some(1),
        });
        assert_eq!(attested.shown(), "attested: secure-enclave (development)");

        let dir = tempfile::tempdir().unwrap();
        let devices = Devices::new(dir.path().to_path_buf());
        let id = devices
            .add(&DeviceKey::generate().public(), &attested)
            .unwrap();
        assert_eq!(devices.info(&id).unwrap(), attested);

        let old =
            r#"{"kind": "secure-enclave", "name": "phone", "paired_at": "2026-10-30T12:00:00Z"}"#;
        let old: DeviceInfo = serde_json::from_str(old).unwrap();
        assert_eq!(old.attestation, None);
        assert!(!serde_json::to_string(&old).unwrap().contains("attestation"));
    }

    #[test]
    fn add_new_keeps_the_first_record() {
        let dir = tempfile::tempdir().unwrap();
        let devices = Devices::new(dir.path().to_path_buf());
        let phone = DeviceKey::generate();
        let first = info(DeviceKind::SecureEnclave, "2026-10-30T12:00:00Z");
        let id = devices.add(&phone.public(), &first).unwrap();
        let again = devices
            .add_new(
                &phone.public(),
                &info(DeviceKind::Browser, "2026-10-31T12:00:00Z"),
            )
            .unwrap();
        assert_eq!(again, id);
        assert_eq!(devices.info(&id).unwrap(), first);

        let legacy = DeviceKey::generate();
        let legacy_id = TrustedKeys::add(dir.path(), &legacy.public()).unwrap();
        devices
            .add_new(
                &legacy.public(),
                &info(DeviceKind::Browser, "2026-10-31T12:00:00Z"),
            )
            .unwrap();
        assert!(devices.info(&legacy_id).is_none());

        let browser = DeviceKey::generate();
        let browser_id = devices
            .add_new(
                &browser.public(),
                &info(DeviceKind::Browser, "2026-10-31T12:00:00Z"),
            )
            .unwrap();
        assert_eq!(devices.info(&browser_id).unwrap().kind, DeviceKind::Browser);
    }
}
