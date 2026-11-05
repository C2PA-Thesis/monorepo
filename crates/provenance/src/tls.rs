//! The certificate the phone app reaches the laptop with on the local network.
//!
//! The app does not check it against a certificate authority: it pins the
//! SHA-256 of the key's SubjectPublicKeyInfo, which it reads from the QR code
//! on the presenter page. Only the key is long lived. The certificate is
//! issued again from it whenever `serve` starts, for the addresses the laptop
//! has at that moment, so moving to another network keeps the pin.

use std::{
    fs,
    net::{IpAddr, Ipv4Addr},
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{anyhow, Context, Result};
use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair, PublicKeyData};
use rustls::{
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
    ServerConfig,
};
use sha2::{Digest, Sha256};

use crate::capture::write_private;

const KEY: &str = "key.pem";
const CERTIFICATE: &str = "cert.pem";

/// What the TLS listener serves, and what the QR code tells the app.
pub struct Tls {
    pub config: Arc<ServerConfig>,
    /// SHA-256 of the SubjectPublicKeyInfo DER, hex.
    pub pin: String,
    /// The certificate's IP addresses, the ones the app can try.
    pub addresses: Vec<Ipv4Addr>,
}

pub struct TlsFiles {
    dir: PathBuf,
}

impl TlsFiles {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// Only the key: `load` issues the certificate again, so a missing one
    /// must not make `setup` replace the key and so the pin.
    pub fn exists(&self) -> bool {
        self.dir.join(KEY).exists()
    }

    /// Creates the key and a first certificate.
    pub fn create(&self) -> Result<()> {
        fs::create_dir_all(&self.dir)
            .with_context(|| format!("creating {}", self.dir.display()))?;
        let key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256)?;
        write_private(&self.dir.join(KEY), key.serialize_pem().as_bytes())?;
        self.issue(&key, &lan_addresses()?).map(drop)
    }

    /// Issues a certificate for the current addresses and builds the listener's config.
    pub fn load(&self) -> Result<Tls> {
        let path = self.dir.join(KEY);
        let pem = fs::read_to_string(&path)
            .with_context(|| format!("reading {}; run `provenance setup` first", path.display()))?;
        let key =
            KeyPair::from_pem(&pem).map_err(|error| anyhow!("{}: {error}", path.display()))?;
        let addresses = lan_addresses()?;
        let certificate = self.issue(&key, &addresses)?;
        let mut config =
            ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_safe_default_protocol_versions()?
                .with_no_client_auth()
                .with_single_cert(
                    vec![certificate],
                    PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der())),
                )?;
        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        Ok(Tls {
            config: Arc::new(config),
            pin: pin(&key),
            addresses,
        })
    }

    fn issue(&self, key: &KeyPair, addresses: &[Ipv4Addr]) -> Result<CertificateDer<'static>> {
        let mut names: Vec<String> = addresses.iter().map(ToString::to_string).collect();
        names.push("localhost".to_string());
        let mut params = CertificateParams::new(names)?;
        let mut subject = DistinguishedName::new();
        subject.push(DnType::CommonName, "provenance serve");
        params.distinguished_name = subject;
        let certificate = params.self_signed(key)?;
        fs::write(self.dir.join(CERTIFICATE), certificate.pem())
            .with_context(|| format!("writing {}", self.dir.join(CERTIFICATE).display()))?;
        Ok(certificate.der().clone())
    }
}

/// The value the app pins: SHA-256 of the SubjectPublicKeyInfo DER, hex.
pub fn pin(key: &impl PublicKeyData) -> String {
    hex::encode(Sha256::digest(key.subject_public_key_info()))
}

/// IPv4 addresses a phone on the same network can reach: not loopback and
/// not link-local, sorted.
pub fn lan_addresses() -> Result<Vec<Ipv4Addr>> {
    let mut addresses: Vec<Ipv4Addr> = if_addrs::get_if_addrs()
        .context("listing network interfaces")?
        .into_iter()
        .filter_map(|interface| match interface.ip() {
            IpAddr::V4(ip) if !ip.is_loopback() && !ip.is_link_local() => Some(ip),
            _ => None,
        })
        .collect();
    addresses.sort_unstable();
    addresses.dedup();
    Ok(addresses)
}

/// Where `setup` keeps the key and certificate.
pub fn dir(home: &Path) -> PathBuf {
    home.join("tls")
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use p256::pkcs8::{DecodePrivateKey, EncodePublicKey};

    use super::*;

    #[test]
    fn the_pin_survives_a_new_certificate_and_matches_the_spki() {
        let dir = tempfile::tempdir().unwrap();
        let files = TlsFiles::new(dir.path().join("tls"));
        files.create().unwrap();
        assert!(files.exists());
        let mode = fs::metadata(dir.path().join("tls/key.pem"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);

        let first = files.load().unwrap();
        let second = files.load().unwrap();
        assert_eq!(first.pin, second.pin);

        // The same digest computed from the key file by another library.
        let pem = fs::read_to_string(dir.path().join("tls/key.pem")).unwrap();
        let secret = p256::SecretKey::from_pkcs8_pem(&pem).unwrap();
        let spki = secret.public_key().to_public_key_der().unwrap();
        assert_eq!(first.pin, hex::encode(Sha256::digest(spki.as_bytes())));
    }

    #[test]
    fn a_missing_certificate_keeps_the_key_and_the_pin() {
        let dir = tempfile::tempdir().unwrap();
        let files = TlsFiles::new(dir.path().join("tls"));
        files.create().unwrap();
        let before = files.load().unwrap().pin;

        fs::remove_file(dir.path().join("tls").join(CERTIFICATE)).unwrap();
        assert!(files.exists());
        assert_eq!(files.load().unwrap().pin, before);
        assert!(dir.path().join("tls").join(CERTIFICATE).exists());
    }
}
