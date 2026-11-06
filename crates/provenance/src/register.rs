//! Registration without pairing, for a server the phone cannot scan a QR
//! code from. The app asks for a challenge, attests its App Attest key over
//! the challenge and the receipt key's id, and the server trusts the receipt
//! key when Apple's attestation verifies. With a DeviceCheck key the server
//! also asks Apple for the risk metric and refuses a device with too many
//! attested keys.
//!
//! What a registered key means differs from a paired one: an instance of the
//! team's app on Apple hardware registered it, not someone at the server.

use std::{
    collections::{HashMap, VecDeque},
    error::Error as StdError,
    fmt,
    sync::{Mutex, MutexGuard, PoisonError},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::Result;
use app_attest::{
    jwt::DeviceCheckKey,
    receipt::{apple_root_g3, verify_receipt_with_root, ReceiptKind},
    Attestation, Environment,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};

use crate::{
    devices::DeviceKind,
    serve::{AppAttestation, ParamDigests},
};

/// How long a challenge stays valid.
pub const CHALLENGE_TTL: Duration = Duration::from_secs(5 * 60);

/// Pending challenges, so asking for them cannot fill the memory.
pub const MAX_CHALLENGES: usize = 1000;

/// Validation categories a registration accepts: TestFlight (2), a
/// development signing identity (3) and the App Store (4), from the table in
/// "Validating apps that connect to your server". An attestation without the
/// extension is accepted and recorded without a category.
pub const ACCEPTED_CATEGORIES: [u32; 3] = [2, 3, 4];

/// The default ceiling on the risk metric, "the number of attested keys
/// associated with a given device over the past 30 days". A first guess, to
/// be tuned with real devices.
pub const MAX_RISK_METRIC: u32 = 10;

/// "Verify that the receipt's creation time, given in field 12, is no more
/// than five minutes old" (Assessing fraud risk, Verify the receipt).
pub const RECEIPT_MAX_AGE: i64 = 5 * 60;

/// Uploads a device may hand in per hour while registration is open. Each
/// one starts a crop proof, the heaviest thing the server does.
pub const UPLOADS_PER_HOUR: usize = 10;

/// Captures waiting for or running a proof, beyond which uploads are refused
/// while registration is open.
pub const MAX_QUEUED: usize = 4;

/// What the app attests when it registers. The prefix differs from
/// pairing's, so an attestation made for one route fails on the other.
pub fn registration_client_data(challenge: &str, device: &str) -> Vec<u8> {
    format!("io.github.c2pa-thesis.register.v1\n{challenge}\n{device}").into_bytes()
}

/// `POST /api/v2/challenge`.
#[derive(Debug, Serialize, Deserialize)]
pub struct Challenge {
    /// 32 random bytes, base64.
    pub challenge: String,
    pub expires_in: u64,
}

/// `POST /api/v2/register`.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegisterRequest {
    pub challenge: String,
    /// P-256 public key in PEM SubjectPublicKeyInfo.
    pub public_key: String,
    /// Base64 DER ECDSA-SHA256 signature over the challenge's UTF-8 bytes.
    pub signature: String,
    /// Only `secure-enclave` registers.
    pub kind: DeviceKind,
    pub name: String,
    /// Over SHA-256 of [`registration_client_data`].
    pub attestation: AppAttestation,
}

/// `GET /api/v2/server`: what the QR code carries apart from the token. The
/// app reaches it over the pinned connection, so it trusts the digests.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerInfo {
    pub v: u32,
    pub urls: Vec<String>,
    pub params: ParamDigests,
    /// Whether `/api/v2/register` is open.
    pub registration: bool,
}

/// A refusal that should be answered with 429.
#[derive(Debug)]
pub struct TooMany(pub String);

impl fmt::Display for TooMany {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl StdError for TooMany {}

/// Exchanges the receipt from an attestation for one with the risk metric.
pub trait MetricSource: Send + Sync {
    fn fetch(&self, receipt: &[u8], environment: Environment) -> Result<Vec<u8>, String>;
}

/// Apple's `attestationData` endpoint.
pub struct AppleMetric {
    key: DeviceCheckKey,
    agent: ureq::Agent,
}

impl AppleMetric {
    pub fn new(key: DeviceCheckKey) -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(15)))
            .http_status_as_error(false)
            .build()
            .into();
        Self { key, agent }
    }

    pub fn key_id(&self) -> &str {
        self.key.key_id()
    }
}

impl MetricSource for AppleMetric {
    fn fetch(&self, receipt: &[u8], environment: Environment) -> Result<Vec<u8>, String> {
        // "You can't use a receipt generated in one environment to request a
        // new receipt in the other" (Assessing fraud risk, Create a request).
        let base = match environment {
            Environment::Development => "https://data-development.appattest.apple.com",
            Environment::Production => "https://data.appattest.apple.com",
        };
        let mut response = self
            .agent
            .post(format!("{base}/v1/attestationData"))
            .header("Authorization", self.key.token(unix_now()))
            .send(BASE64.encode(receipt))
            .map_err(|error| format!("asking Apple for the risk metric: {error}"))?;
        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .with_config()
            .limit(64 * 1024)
            .read_to_string()
            .unwrap_or_default();
        if status != 200 {
            return Err(format!(
                "Apple answered the risk metric request with {status} {}",
                body.trim()
            ));
        }
        BASE64
            .decode(body.trim())
            .map_err(|_| "Apple's receipt is not base64".to_string())
    }
}

/// The server's half of registration.
pub struct Registration {
    challenges: Mutex<Challenges>,
    metric: Option<Box<dyn MetricSource>>,
    max_risk_metric: u32,
    receipt_root: Vec<u8>,
    uploads: Mutex<HashMap<String, VecDeque<Instant>>>,
}

impl Registration {
    /// Without `metric` the risk metric is not checked.
    pub fn new(metric: Option<Box<dyn MetricSource>>, max_risk_metric: u32) -> Self {
        Self {
            challenges: Mutex::new(Challenges::new(CHALLENGE_TTL)),
            metric,
            max_risk_metric,
            receipt_root: apple_root_g3(),
            uploads: Mutex::new(HashMap::new()),
        }
    }

    /// Trusts another receipt root instead of Apple Root CA - G3. Only for tests.
    #[doc(hidden)]
    pub fn receipt_root(mut self, root: Vec<u8>) -> Self {
        self.receipt_root = root;
        self
    }

    /// How long challenges live, shorter in tests.
    pub fn challenge_ttl(self, ttl: Duration) -> Self {
        *self.challenges() = Challenges::new(ttl);
        self
    }

    pub fn checks_risk_metric(&self) -> bool {
        self.metric.is_some()
    }

    pub fn max_risk_metric(&self) -> u32 {
        self.max_risk_metric
    }

    pub fn challenge(&self) -> Result<Challenge> {
        let (challenge, expires_in) = self.challenges().issue()?;
        Ok(Challenge {
            challenge,
            expires_in: expires_in.as_secs(),
        })
    }

    /// Spends `challenge`. False when it is unknown, used or expired.
    pub(crate) fn take(&self, challenge: &str) -> bool {
        self.challenges().take(challenge)
    }

    /// The category policy, on an attestation that verified.
    pub(crate) fn check_category(&self, attestation: &Attestation) -> Result<(), String> {
        match attestation.validation_category {
            Some(category) if !ACCEPTED_CATEGORIES.contains(&category) => Err(format!(
                "validation category {category} does not register; TestFlight (2), development (3) and App Store (4) do"
            )),
            _ => Ok(()),
        }
    }

    /// Apple's risk metric for the device behind `attestation`, refused above
    /// the ceiling. None when this server has no DeviceCheck key.
    pub(crate) fn risk_metric(
        &self,
        app_id: &str,
        attestation: &Attestation,
        now: i64,
    ) -> Result<Option<u32>, String> {
        let Some(metric) = &self.metric else {
            return Ok(None);
        };
        let der = metric.fetch(&attestation.receipt, attestation.environment)?;
        let receipt = verify_receipt_with_root(&der, now, &self.receipt_root)
            .map_err(|error| format!("Apple's receipt: {error}"))?;
        if receipt.kind != ReceiptKind::Receipt {
            return Err("Apple's receipt is not of type RECEIPT".to_string());
        }
        if receipt.app_id != app_id {
            return Err(format!(
                "Apple's receipt is for {}, not {app_id}",
                receipt.app_id
            ));
        }
        if now - receipt.created > RECEIPT_MAX_AGE || receipt.created - now > 60 {
            return Err("Apple's receipt is more than five minutes old".to_string());
        }
        if !receipt.attests(&attestation.public_key) {
            return Err("Apple's receipt is for another App Attest key".to_string());
        }
        let count = receipt
            .risk_metric
            .ok_or("Apple's receipt carries no risk metric")?;
        if count > self.max_risk_metric {
            return Err(format!(
                "Apple counts {count} attested keys on this device in 30 days, above {}",
                self.max_risk_metric
            ));
        }
        Ok(Some(count))
    }

    /// Counts an upload from `device`, refusing it past [`UPLOADS_PER_HOUR`].
    pub(crate) fn admit_upload(&self, device: &str) -> Result<(), TooMany> {
        let mut uploads = self.uploads.lock().unwrap_or_else(PoisonError::into_inner);
        let hour = Duration::from_secs(3600);
        uploads.retain(|_, times| {
            while times.front().is_some_and(|t| t.elapsed() >= hour) {
                times.pop_front();
            }
            !times.is_empty()
        });
        let times = uploads.entry(device.to_string()).or_default();
        if times.len() >= UPLOADS_PER_HOUR {
            return Err(TooMany(format!(
                "this device sent {UPLOADS_PER_HOUR} captures in the last hour; wait and try again"
            )));
        }
        times.push_back(Instant::now());
        Ok(())
    }

    fn challenges(&self) -> MutexGuard<'_, Challenges> {
        self.challenges
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

struct Challenges {
    pending: HashMap<String, Instant>,
    ttl: Duration,
}

impl Challenges {
    fn new(ttl: Duration) -> Self {
        Self {
            pending: HashMap::new(),
            ttl,
        }
    }

    fn issue(&mut self) -> Result<(String, Duration)> {
        let ttl = self.ttl;
        self.pending.retain(|_, issued| issued.elapsed() < ttl);
        if self.pending.len() >= MAX_CHALLENGES {
            return Err(TooMany(
                "too many registrations at once; try again in a few minutes".into(),
            )
            .into());
        }
        let mut bytes = [0u8; 32];
        OsRng.fill_bytes(&mut bytes);
        let challenge = BASE64.encode(bytes);
        self.pending.insert(challenge.clone(), Instant::now());
        Ok((challenge, ttl))
    }

    fn take(&mut self, challenge: &str) -> bool {
        self.pending
            .remove(challenge)
            .is_some_and(|issued| issued.elapsed() < self.ttl)
    }
}

pub(crate) fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::Digest;

    #[test]
    fn a_challenge_is_spent_once_and_expires() {
        let registration = Registration::new(None, MAX_RISK_METRIC);
        let challenge = registration.challenge().unwrap().challenge;
        assert_eq!(BASE64.decode(&challenge).unwrap().len(), 32);
        assert!(registration.take(&challenge));
        assert!(!registration.take(&challenge));
        assert!(!registration.take("made up"));

        let registration = registration.challenge_ttl(Duration::ZERO);
        let challenge = registration.challenge().unwrap().challenge;
        assert!(!registration.take(&challenge));
    }

    #[test]
    fn pending_challenges_are_capped() {
        let registration = Registration::new(None, MAX_RISK_METRIC);
        for _ in 0..MAX_CHALLENGES {
            registration.challenge().unwrap();
        }
        let refused = registration.challenge().unwrap_err();
        assert!(refused.is::<TooMany>());
    }

    #[test]
    fn uploads_are_counted_per_device() {
        let registration = Registration::new(None, MAX_RISK_METRIC);
        for _ in 0..UPLOADS_PER_HOUR {
            registration.admit_upload("aa").unwrap();
        }
        assert!(registration.admit_upload("aa").is_err());
        registration.admit_upload("bb").unwrap();
    }

    #[test]
    fn registration_client_data_is_pinned() {
        // The app pins the same bytes in its tests.
        assert_eq!(
            hex::encode(sha2::Sha256::digest(registration_client_data(
                "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
                &"ab".repeat(32),
            ))),
            "aa957f1bc74a76d9efaab134e994051e74b58b8a87e0d0c6fae61983cc768543"
        );
    }
}
