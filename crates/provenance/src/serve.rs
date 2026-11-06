//! The HTTP API the capture page, the app and the presenter page talk to:
//! pair a device key, look up the cell for a coordinate, hand in a capture
//! the phone signed, follow it while the laptop publishes it, and verify any
//! file.
//!
//! An upload is checked the way a reader would check it before it is stored:
//! the fingerprint must match the pixels, the receipt must be signed by a
//! trusted key, and the location must hold. From the capture page that means
//! the envelope matches the secrets and the cell is the one the location
//! circuit maps the coordinate to. The app proves the location itself
//! (`/api/v2/captures`), so the laptop verifies that proof against the
//! envelope and never sees the coordinate.
//!
//! The server listens twice. The plain listener on 127.0.0.1 runs behind a
//! tunnel for the capture page, so every route is reachable from the internet
//! unless it asks for [`Local`]. The TLS listener faces the local network for
//! the app, which pins its key. Nothing private (the original, the secrets,
//! the pairing secrets) is served to a request from either.

use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Write,
    net::{Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    thread,
    time::{Duration, Instant},
};

use anyhow::{anyhow, bail, ensure, Context, Result};
use axum::{
    body::{Body, Bytes},
    extract::{
        ConnectInfo, DefaultBodyLimit, FromRequest, FromRequestParts, Path as UrlPath, Request,
        State,
    },
    http::{header, request::Parts, StatusCode},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use crop_proof::{Fingerprint, Rect, RgbImage, Size};
use p256::ecdsa::{signature::Verifier as _, Signature};
use rand::{rngs::OsRng, Rng, RngCore};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tower::ServiceExt;
use tower_http::{
    services::{ServeDir, ServeFile},
    set_header::SetResponseHeaderLayer,
};

use crate::{
    capture::{self, Capture, Coordinate, LocationSource, Secrets, Source, Summary, Witness},
    devices::{DeviceInfo, DeviceKind},
    image,
    location::{check_resolution, LocationProof, Outline, Region, PROVER_FILES},
    publish,
    receipt::{parse_public_key, Receipt, TrustedKeys},
    verify::Verdict,
    Workspace,
};

/// A PNG of the original plus its JSON is well under this.
const MAX_BODY_BYTES: usize = 8 * 1024 * 1024;

/// How many captures the presenter page lists, newest first.
const LISTED: usize = 50;

/// How long an app pairing token stays valid.
pub const PAIRING_TTL: Duration = Duration::from_secs(5 * 60);

/// Refused app pairings, counted across tokens, before the token is replaced.
/// Also how many wrong capture page codes start a [`CODE_COOLDOWN`].
pub const PAIRING_ATTEMPTS: u32 = 5;

/// How long the capture page pairing refuses every code after
/// [`PAIRING_ATTEMPTS`] wrong ones. The code never changes and is reachable
/// through the tunnel, so this is what keeps it from being guessed: at five
/// guesses a minute, half of the million codes take about 70 days.
pub const CODE_COOLDOWN: Duration = Duration::from_secs(60);

/// The default port of the TLS listener for the app.
pub const TLS_PORT: u16 = 8792;

/// Header of `/api/v2/pairing`: seconds until its token expires, so the body
/// stays exactly the QR payload.
pub const EXPIRES_IN: &str = "x-expires-in";

type Listener = Box<dyn Fn(&Status) + Send + Sync>;

pub struct Server {
    workspace: Workspace,
    captures: PathBuf,
    /// Shown on the laptop and typed on the phone, so only someone at the
    /// laptop can add a trusted key through the tunnel.
    pairing_code: String,
    code_attempts: Mutex<Cooldown>,
    /// The app's counterpart to the pairing code, shown only in the QR code.
    pairing: Mutex<Pairing>,
    /// The tunnel's address, for the QR code on the presenter page.
    public_url: Option<String>,
    /// How the app reaches the TLS listener, when it runs.
    lan: Option<Lan>,
    /// SHA-256 of each prover file, computed on first use.
    params: Mutex<Option<ParamDigests>>,
    /// Whether an accepted upload is published right away.
    publishes: bool,
    /// Captures this process has published or is publishing, by id.
    jobs: Mutex<HashMap<String, Status>>,
    /// One proof at a time: the crop proof already uses every core.
    prover: Mutex<()>,
    on_change: Option<Listener>,
    /// The laptop's own key id, which signs the demo's captures.
    laptop: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PairRequest {
    pub code: String,
    /// P-256 public key in PEM.
    pub public_key: String,
}

#[derive(Serialize)]
pub struct Paired {
    pub device: String,
}

/// The app pairs with a token from the QR code and shows that it holds the
/// private key by signing the token.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppPairRequest {
    pub token: String,
    /// P-256 public key in PEM SubjectPublicKeyInfo.
    pub public_key: String,
    /// Base64 DER ECDSA-SHA256 signature over the token's UTF-8 bytes.
    pub signature: String,
    /// `secure-enclave` or `software`, as the app declares it. Nothing checks
    /// where the key lives.
    pub kind: DeviceKind,
    pub name: String,
}

/// The TLS listener's addresses and the pin of its key.
#[derive(Clone, Debug)]
pub struct Lan {
    /// `https://<lan-ip>:<port>`, one per address.
    pub urls: Vec<String>,
    /// SHA-256 of the SubjectPublicKeyInfo DER, hex.
    pub pin: String,
}

/// The addresses the app's QR code names: `extra` first, then one per
/// interface address. `extra` is for addresses no interface carries, like a
/// cloud instance's public IP behind NAT. The app pins the key instead of
/// checking the host name, so any host that reaches the listener works.
pub fn app_urls(addresses: &[Ipv4Addr], port: u16, extra: &[String]) -> Result<Vec<String>> {
    let mut urls = Vec::with_capacity(extra.len() + addresses.len());
    for url in extra {
        let host = url
            .strip_prefix("https://")
            .and_then(|rest| rest.split(['/', ':']).next())
            .unwrap_or_default();
        ensure!(
            !host.is_empty() && !url.contains(char::is_whitespace),
            "app URL {url:?} is not https://<host>[:port]"
        );
        urls.push(url.trim_end_matches('/').to_string());
    }
    urls.extend(
        addresses
            .iter()
            .map(|address| format!("https://{address}:{port}")),
    );
    let mut seen = HashSet::new();
    urls.retain(|url| seen.insert(url.clone()));
    Ok(urls)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParamDigests {
    #[serde(rename = "proving.key")]
    pub proving_key: String,
    #[serde(rename = "circuit.r1cs")]
    pub circuit: String,
}

/// What the app's QR code holds, and what `GET /api/v2/pairing` returns.
/// Field order is the wire order.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppQr {
    pub v: u32,
    pub urls: Vec<String>,
    pub pin: String,
    pub token: String,
    pub params: ParamDigests,
}

/// `GET /api/v2/session`: the payload with its expiry in the body.
#[derive(Debug, Serialize, Deserialize)]
pub struct AppSession {
    /// Encoded as JSON in the QR code, as is.
    pub qr: AppQr,
    /// Seconds until the token expires and is replaced.
    pub expires_in: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CellRequest {
    pub latitude: f64,
    pub longitude: f64,
    pub resolution: u8,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Upload {
    /// Base64 PNG of the original the device fingerprinted, up to 2^19 pixels.
    pub original_png: String,
    pub secrets: Secrets,
    pub receipt: Receipt,
    pub cell: String,
    pub resolution: u8,
    /// The rectangle the photographer chose to publish.
    pub crop: Rect,
    pub accuracy_meters: Option<f64>,
}

/// A capture from the app, which proved the location on the phone. It
/// carries no secrets.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProofUpload {
    /// Base64 PNG of the original the device fingerprinted, up to 2^19 pixels.
    pub original_png: String,
    pub receipt: Receipt,
    pub cell: String,
    pub resolution: u8,
    pub crop: Rect,
    pub accuracy_meters: Option<f64>,
    pub location_proof: PhoneProof,
    #[serde(default)]
    pub location: Option<LocationSource>,
}

/// The base64 Groth16 proof, or the `{envelope, proof}` the Go library
/// returns. The envelope, when present, must be the receipt's.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PhoneProof {
    Proof(String),
    WithEnvelope(LocationProof),
}

#[derive(Serialize, Deserialize)]
pub struct Accepted {
    pub id: String,
    pub dir: PathBuf,
}

/// Where a capture is on its way from upload to a verified file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    /// Stored, not published.
    Stored,
    /// Published by an earlier run; not checked by this one.
    Published,
    /// Waiting for another capture's proof to finish.
    Queued,
    Publishing,
    Verifying,
    /// Published, and every reader check passed.
    Accepted,
    /// Published, and a reader check refused it.
    Rejected,
    /// Publishing stopped with an error.
    Failed,
}

impl Phase {
    pub fn is_running(self) -> bool {
        matches!(self, Phase::Queued | Phase::Publishing | Phase::Verifying)
    }
}

/// The public view of one capture. It carries nothing the published file
/// does not, apart from the GPS accuracy the phone reported.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Status {
    pub id: String,
    pub phase: Phase,
    /// Key id of the device that signed the receipt.
    pub device: String,
    /// What kind of key that is, when known.
    pub device_kind: Option<DeviceKind>,
    pub captured_at: String,
    /// The original's size, from the signed fingerprint.
    pub original: Size,
    pub capture: Summary,
    /// Publish stages, then reader checks, each as `--json` prints it.
    pub events: Vec<Value>,
    /// Why publishing failed, or which check refused the file.
    pub error: Option<String>,
    /// The joint statement, once every check passed.
    pub claim: Option<Value>,
    /// Whether `signed.png` can be downloaded.
    pub published: bool,
}

/// What a reader learns from a file: each check as it ran, then the verdict.
#[derive(Serialize)]
pub struct Report {
    pub events: Vec<Value>,
    pub verdict: Verdict,
}

impl Server {
    pub fn new(workspace: Workspace, captures: PathBuf) -> Self {
        let laptop = workspace.device_key().and_then(|key| key.id()).ok();
        Self {
            workspace,
            captures,
            pairing_code: format!("{:06}", OsRng.gen_range(0..1_000_000)),
            code_attempts: Mutex::new(Cooldown::new(CODE_COOLDOWN)),
            pairing: Mutex::new(Pairing::new(PAIRING_TTL)),
            laptop,
            public_url: None,
            lan: None,
            params: Mutex::new(None),
            publishes: false,
            jobs: Mutex::new(HashMap::new()),
            prover: Mutex::new(()),
            on_change: None,
        }
    }

    /// Publishes and verifies every accepted upload in the background.
    pub fn publishing(mut self) -> Self {
        self.publishes = true;
        self
    }

    pub fn public_url(mut self, url: Option<String>) -> Self {
        self.public_url = url.map(|url| url.trim_end_matches('/').to_string());
        self
    }

    /// Lets the app pair over the TLS listener described by `lan`.
    pub fn lan(mut self, lan: Lan) -> Self {
        self.lan = Some(lan);
        self
    }

    /// How long an app pairing token lives, shorter in tests.
    pub fn pairing_ttl(self, ttl: Duration) -> Self {
        *self.pairing() = Pairing::new(ttl);
        self
    }

    /// Called whenever a capture changes phase.
    pub fn on_change(mut self, listener: impl Fn(&Status) + Send + Sync + 'static) -> Self {
        self.on_change = Some(Box::new(listener));
        self
    }

    pub fn pairing_code(&self) -> &str {
        &self.pairing_code
    }

    /// The link the QR code holds: the capture page, with the pairing code
    /// in the fragment so the phone pairs without typing it. The fragment
    /// never reaches a server, ngrok included.
    pub fn pair_url(&self) -> Option<String> {
        let url = self.public_url.as_ref()?;
        Some(format!("{url}/#pair={}", self.pairing_code))
    }

    /// The API, with `web` served for every other path when given.
    pub fn router(self: Arc<Self>, web: Option<PathBuf>) -> Router {
        let api = Router::new()
            .route("/api/session", get(session))
            .route("/api/pair", post(pair))
            .route("/api/v2/pairing", get(app_pairing))
            .route("/api/v2/session", get(app_session))
            .route("/api/v2/pair", post(app_pair))
            .route("/api/v2/captures", post(proof_upload))
            .route("/api/params/location/{file}", get(params))
            .route("/api/devices/{id}", get(device))
            .route("/api/cell", post(cell))
            .route("/api/cells/{cell}", get(outline))
            .route("/api/captures", post(upload).get(list))
            .route("/api/captures/{id}", get(status))
            .route("/api/captures/{id}/publish", post(republish))
            .route("/api/captures/{id}/signed.png", get(signed))
            .route("/api/captures/{id}/original.png", get(original))
            .route("/api/verify", post(verify))
            .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
            .with_state(self);
        let router = match web {
            Some(dir) => api
                .route("/screen", get(|| async { Redirect::to("/screen.html") }))
                .fallback_service(ServeDir::new(dir)),
            None => api,
        };
        // A phone that cached an older page would sign with older code.
        router.layer(SetResponseHeaderLayer::if_not_present(
            header::CACHE_CONTROL,
            header::HeaderValue::from_static("no-cache"),
        ))
    }

    fn pair(&self, request: PairRequest) -> Result<Paired, ApiError> {
        let right = equal_in_constant_time(request.code.as_bytes(), self.pairing_code.as_bytes());
        if !self
            .code_attempts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .admit(right)
        {
            return Err(ApiError {
                status: StatusCode::TOO_MANY_REQUESTS,
                error: anyhow!(
                    "too many wrong pairing codes; wait a minute and type the code again"
                ),
            });
        }
        if !right {
            return Err(anyhow!("wrong pairing code").into());
        }
        let key = parse_public_key(&request.public_key)?;
        let device = self.workspace.devices().add_new(
            &key,
            &DeviceInfo {
                kind: DeviceKind::Browser,
                name: "capture page".to_string(),
                paired_at: capture::now()?,
            },
        )?;
        Ok(Paired { device })
    }

    /// What the presenter page puts in the app's QR code. An expired token
    /// is replaced here, so the page always shows one that works.
    pub fn app_session(&self) -> Result<AppSession> {
        let lan = self
            .lan
            .as_ref()
            .context("the TLS listener for the app is not running")?;
        let params = self.param_digests()?;
        let (token, expires_in) = self.pairing().current();
        Ok(AppSession {
            qr: AppQr {
                v: 1,
                urls: lan.urls.clone(),
                pin: lan.pin.clone(),
                token,
                params,
            },
            expires_in: expires_in.as_secs(),
        })
    }

    fn app_pair(&self, request: AppPairRequest) -> Result<Paired> {
        ensure!(
            matches!(
                request.kind,
                DeviceKind::SecureEnclave | DeviceKind::Software
            ),
            "kind must be secure-enclave or software"
        );
        let key = parse_public_key(&request.public_key)?;
        let possession = BASE64
            .decode(&request.signature)
            .ok()
            .and_then(|der| Signature::from_der(&der).ok())
            .is_some_and(|signature| key.verify(request.token.as_bytes(), &signature).is_ok());
        self.pairing().redeem(&request.token, possession)?;
        let device = self.workspace.devices().add(
            &key,
            &DeviceInfo {
                kind: request.kind,
                name: device_name(&request.name),
                paired_at: capture::now()?,
            },
        )?;
        Ok(Paired { device })
    }

    fn pairing(&self) -> MutexGuard<'_, Pairing> {
        self.pairing.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn param_digests(&self) -> Result<ParamDigests> {
        let mut cached = self.params.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(digests) = cached.as_ref() {
            return Ok(digests.clone());
        }
        let dir = self.workspace.location_tool().params().to_path_buf();
        let digest = |file: &str| -> Result<String> {
            let path = dir.join(file);
            let mut reader = fs::File::open(&path).with_context(|| {
                format!("reading {}; run `provenance setup` first", path.display())
            })?;
            let mut hasher = Sha256::new();
            std::io::copy(&mut reader, &mut hasher)?;
            Ok(hex::encode(hasher.finalize()))
        };
        let digests = ParamDigests {
            proving_key: digest(PROVER_FILES[0])?,
            circuit: digest(PROVER_FILES[1])?,
        };
        *cached = Some(digests.clone());
        Ok(digests)
    }

    fn is_trusted(&self, device: &str) -> Result<bool> {
        ensure!(
            !device.is_empty() && device.chars().all(|c| c.is_ascii_hexdigit()),
            "a device id is hex"
        );
        Ok(self.workspace.devices().is_trusted(device))
    }

    /// What kind of key signed a capture: the sidecar's, or a guess for keys
    /// trusted before sidecars existed, when only the laptop and the capture
    /// page paired. The laptop's key is known either way, as `devices` knows it.
    fn device_kind(&self, device: &str, source: Source) -> Option<DeviceKind> {
        if let Some(info) = self.workspace.devices().info(device) {
            return Some(info.kind);
        }
        if self.laptop.as_deref() == Some(device) {
            return Some(DeviceKind::Laptop);
        }
        match source {
            Source::Simulated => Some(DeviceKind::Laptop),
            Source::Phone => Some(DeviceKind::Browser),
        }
    }

    fn cell(&self, request: CellRequest) -> Result<Region> {
        let coordinate = Coordinate {
            latitude: request.latitude,
            longitude: request.longitude,
        };
        self.workspace
            .location_tool()
            .cell(coordinate, request.resolution)
    }

    /// The original, once the receipt signature and its fingerprint check out.
    /// The signature goes first: fingerprinting is seconds of work that an
    /// unauthenticated request through the tunnel should not get for free.
    fn signed_original(&self, original_png: &str, receipt: &Receipt) -> Result<RgbImage> {
        receipt.verify(&TrustedKeys::load(&self.workspace.trusted_keys_dir())?)?;
        let png = BASE64
            .decode(original_png)
            .context("original_png is not base64")?;
        let original = image::decode_png(&png)?;
        ensure!(
            Fingerprint::of(&original)? == receipt.fingerprint,
            "the fingerprint does not match the pixels"
        );
        Ok(original)
    }

    /// Checks the upload as a reader would and stores it as a capture directory.
    fn accept(self: &Arc<Self>, upload: Upload) -> Result<Accepted> {
        let original = self.signed_original(&upload.original_png, &upload.receipt)?;
        let tool = self.workspace.location_tool();
        ensure!(
            tool.commit(&upload.secrets)? == upload.receipt.envelope,
            "the envelope does not match the secrets"
        );
        upload.crop.check(original.size())?;
        let region = tool.cell(upload.secrets.coordinate(), upload.resolution)?;
        ensure!(
            region.cell == upload.cell,
            "the coordinate is in cell {} at resolution {}, not {}",
            region.cell,
            upload.resolution,
            upload.cell
        );

        self.store(Capture {
            original,
            witness: Witness::Secrets(upload.secrets),
            receipt: upload.receipt,
            summary: Summary {
                source: Source::Phone,
                cell: upload.cell,
                resolution: upload.resolution,
                crop: upload.crop,
                accuracy_meters: upload.accuracy_meters,
                location: None,
            },
        })
    }

    /// Checks an upload from the app, which carries a location proof instead
    /// of secrets, and stores it.
    fn accept_proof(self: &Arc<Self>, upload: ProofUpload) -> Result<Accepted> {
        let original = self.signed_original(&upload.original_png, &upload.receipt)?;
        check_resolution(upload.resolution)?;
        upload.crop.check(original.size())?;
        let tool = self.workspace.location_tool();
        let region = tool.region(&upload.cell)?;
        ensure!(
            region.resolution == upload.resolution,
            "cell {} is at resolution {}, not {}",
            upload.cell,
            region.resolution,
            upload.resolution
        );
        let envelope = &upload.receipt.envelope;
        let proof = match upload.location_proof {
            PhoneProof::Proof(proof) => LocationProof {
                envelope: envelope.clone(),
                proof,
            },
            PhoneProof::WithEnvelope(proof) => {
                ensure!(
                    &proof.envelope == envelope,
                    "the location proof is for envelope {}, not the receipt's {envelope}",
                    proof.envelope
                );
                proof
            }
        };
        tool.verify(&upload.cell, envelope, &proof.proof).context(
            "the location proof does not verify for the receipt's envelope and this cell",
        )?;
        self.store(Capture {
            original,
            witness: Witness::Proof(proof),
            receipt: upload.receipt,
            summary: Summary {
                source: Source::Phone,
                cell: upload.cell,
                resolution: upload.resolution,
                crop: upload.crop,
                accuracy_meters: upload.accuracy_meters,
                location: upload.location,
            },
        })
    }

    fn store(self: &Arc<Self>, capture: Capture) -> Result<Accepted> {
        let id = capture_id(&capture.receipt);
        let dir = self.captures.join(&id);
        ensure!(!dir.exists(), "capture {id} already exists");
        capture.write(&dir)?;
        if self.publishes {
            self.start(&id).map_err(|rejection| rejection.error)?;
        }
        Ok(Accepted { id, dir })
    }

    /// The capture directory for `id`, refusing anything that is not one.
    fn dir(&self, id: &str) -> Result<PathBuf, ApiError> {
        let valid = !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
        let dir = self.captures.join(id);
        match valid && dir.join("receipt.json").exists() {
            true => Ok(dir),
            false => Err(ApiError::not_found(format!("no capture {id}"))),
        }
    }

    fn status(&self, id: &str) -> Result<Status, ApiError> {
        if let Some(status) = self.jobs().get(id) {
            return Ok(status.clone());
        }
        let dir = self.dir(id)?;
        let (receipt, capture) = Capture::read_public(&dir)?;
        let published = dir.join("signed.png").exists();
        Ok(Status {
            id: id.to_string(),
            phase: if published {
                Phase::Published
            } else {
                Phase::Stored
            },
            device_kind: self.device_kind(&receipt.device, capture.source),
            device: receipt.device,
            captured_at: receipt.captured_at,
            original: receipt.fingerprint.size,
            capture,
            events: Vec::new(),
            error: None,
            claim: None,
            published,
        })
    }

    fn list(&self) -> Result<Vec<Status>> {
        let mut ids: Vec<String> = match fs::read_dir(&self.captures) {
            Ok(entries) => entries
                .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
                .collect(),
            Err(_) => Vec::new(),
        };
        // Ids start with the capture time.
        ids.sort_unstable_by(|a, b| b.cmp(a));
        Ok(ids
            .iter()
            .filter_map(|id| self.status(id).ok())
            .take(LISTED)
            .collect())
    }

    /// Queues `id` for publishing on a thread of its own.
    fn start(self: &Arc<Self>, id: &str) -> Result<Status, ApiError> {
        let mut status = self.status(id)?;
        if status.phase.is_running() {
            return Err(anyhow!("capture {id} is already being published").into());
        }
        status.phase = Phase::Queued;
        status.events.clear();
        status.error = None;
        status.claim = None;
        self.jobs().insert(id.to_string(), status.clone());
        self.notify(&status);

        let server = Arc::clone(self);
        let id = id.to_string();
        thread::spawn(move || server.publish(&id));
        Ok(status)
    }

    fn publish(&self, id: &str) {
        let _turn = self.prover.lock().unwrap_or_else(PoisonError::into_inner);
        self.change(id, |status| status.phase = Phase::Publishing);
        let outcome = (|| -> Result<Verdict> {
            let dir = self.captures.join(id);
            let capture = Capture::read(&dir)?;
            let signed = publish::run(&self.workspace, &capture, &dir, &mut |event| {
                self.change(id, |status| status.events.push(event_json(&event)))
            })?;
            self.change(id, |status| {
                status.phase = Phase::Verifying;
                status.published = true;
            });
            Ok(self.workspace.verifier()?.verify(
                &signed,
                Some(&capture.summary.cell),
                &mut |event| self.change(id, |status| status.events.push(event_json(&event))),
            ))
        })();
        self.change(id, |status| match outcome {
            Ok(verdict) => match verdict.rejected {
                Some(step) => {
                    status.phase = Phase::Rejected;
                    status.error = Some(format!("{}: {}", step.check.title(), step.detail));
                }
                None => {
                    status.phase = Phase::Accepted;
                    status.claim = verdict.claim.as_ref().map(event_json);
                }
            },
            Err(error) => {
                status.phase = Phase::Failed;
                status.error = Some(format!("{error:#}"));
            }
        });
    }

    fn change(&self, id: &str, update: impl FnOnce(&mut Status)) {
        let mut jobs = self.jobs();
        let Some(status) = jobs.get_mut(id) else {
            return;
        };
        let before = status.phase;
        update(status);
        if status.phase != before {
            let status = status.clone();
            drop(jobs);
            self.notify(&status);
        }
    }

    fn notify(&self, status: &Status) {
        if let Some(listener) = &self.on_change {
            listener(status);
        }
    }

    fn jobs(&self) -> MutexGuard<'_, HashMap<String, Status>> {
        self.jobs.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Runs the reader checks on `png`, as `provenance verify` would.
    fn verify(&self, png: &[u8]) -> Result<Report> {
        let mut file = tempfile::Builder::new().suffix(".png").tempfile()?;
        file.write_all(png)?;
        let mut events = Vec::new();
        let verdict = self
            .workspace
            .verifier()?
            .verify(file.path(), None, &mut |event| {
                events.push(event_json(&event))
            });
        Ok(Report { events, verdict })
    }
}

fn event_json(event: &impl Serialize) -> Value {
    serde_json::to_value(event).expect("events serialize")
}

/// Capture time, then a prefix of the fingerprint digest: sortable, and
/// distinct for two captures in the same second.
fn capture_id(receipt: &Receipt) -> String {
    let time: String = receipt
        .captured_at
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect();
    format!("{time}-{}", &receipt.fingerprint_digest()[..8])
}

/// Serves `router` on localhost until the process ends, and on every
/// address with TLS when `tls` gives a config and a port.
pub fn run(router: Router, port: u16, tls: Option<(Arc<rustls::ServerConfig>, u16)>) -> Result<()> {
    tokio::runtime::Runtime::new()?.block_on(async {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
            .await
            .with_context(|| format!("binding port {port}"))?;
        let plain = axum::serve(
            listener,
            router
                .clone()
                .into_make_service_with_connect_info::<SocketAddr>(),
        );
        let Some((config, tls_port)) = tls else {
            return plain.await.context("serving");
        };
        let secure = async move {
            axum_server::bind_rustls(
                SocketAddr::from(([0, 0, 0, 0], tls_port)),
                axum_server::tls_rustls::RustlsConfig::from_config(config),
            )
            .serve(router.into_make_service_with_connect_info::<SocketAddr>())
            .await
            .with_context(|| format!("serving TLS on port {tls_port}"))
        };
        let plain = async { plain.await.context("serving") };
        tokio::try_join!(plain, secure).map(drop)
    })
}

/// A request made on the laptop itself. One that came through the tunnel
/// names the tunnel's host and carries X-Forwarded-For, even though it
/// arrives from 127.0.0.1 like a local one. One that came over the local
/// network arrives from another address, whatever Host it names.
pub struct Local;

impl<S: Send + Sync> FromRequestParts<S> for Local {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        // Both listeners record the peer. A router served without it (a test
        // calling it directly) can only be reached from this process.
        let from_loopback = ConnectInfo::<SocketAddr>::from_request_parts(parts, state)
            .await
            .map_or(true, |ConnectInfo(peer)| peer.ip().is_loopback());
        match from_loopback && is_local(parts) {
            true => Ok(Local),
            false => Err(ApiError {
                status: StatusCode::FORBIDDEN,
                error: anyhow!("only the laptop running provenance serve can open this"),
            }),
        }
    }
}

/// A request that did not come through the tunnel: from the local network,
/// or from this laptop (the Simulator). App pairing asks for it, so refused
/// attempts from the internet cannot replace the token the QR code shows.
pub struct Direct;

impl<S: Send + Sync> FromRequestParts<S> for Direct {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let from_loopback = ConnectInfo::<SocketAddr>::from_request_parts(parts, state)
            .await
            .map_or(true, |ConnectInfo(peer)| peer.ip().is_loopback());
        // The tunnel always forwards a Host; only a test omits it.
        let named = parts.headers.contains_key(header::HOST) || parts.uri.authority().is_some();
        let tunneled = parts.headers.contains_key("x-forwarded-for")
            || (from_loopback && named && !is_local(parts));
        match tunneled {
            false => Ok(Direct),
            true => Err(ApiError {
                status: StatusCode::FORBIDDEN,
                error: anyhow!(
                    "the app pairs over the local network or on this laptop, not through the tunnel"
                ),
            }),
        }
    }
}

fn is_local(parts: &Parts) -> bool {
    // HTTP/2, which the TLS listener offers, names the host in :authority
    // and sends no Host header.
    let host = parts
        .headers
        .get(header::HOST)
        .and_then(|host| host.to_str().ok())
        .or_else(|| parts.uri.authority().map(|authority| authority.as_str()))
        .unwrap_or_default();
    let name = host.rsplit_once(':').map_or(host, |(name, _)| name);
    !parts.headers.contains_key("x-forwarded-for") && matches!(name, "localhost" | "127.0.0.1")
}

type ApiResult<T> = std::result::Result<Json<T>, ApiError>;

/// [`Json`], with a body that does not parse answered as `{"error": "..."}`
/// like every other failure, keeping axum's status.
pub struct JsonBody<T>(T);

impl<S: Send + Sync, T: DeserializeOwned> FromRequest<S> for JsonBody<T> {
    type Rejection = ApiError;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        match Json::<T>::from_request(request, state).await {
            Ok(Json(value)) => Ok(JsonBody(value)),
            Err(rejection) => Err(ApiError {
                status: rejection.status(),
                error: anyhow!(rejection.body_text()),
            }),
        }
    }
}

/// [`Bytes`], with a body over the limit answered as `{"error": "..."}`.
pub struct RawBody(Bytes);

impl<S: Send + Sync> FromRequest<S> for RawBody {
    type Rejection = ApiError;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        match Bytes::from_request(request, state).await {
            Ok(bytes) => Ok(RawBody(bytes)),
            Err(rejection) => Err(ApiError {
                status: rejection.status(),
                error: anyhow!(rejection.body_text()),
            }),
        }
    }
}

async fn session(_: Local, State(server): State<Arc<Server>>) -> Json<Value> {
    Json(json!({
        "pairing_code": server.pairing_code,
        "public_url": server.public_url,
        "pair_url": server.pair_url(),
        "publishes": server.publishes,
        "captures": server.captures,
    }))
}

async fn pair(
    State(server): State<Arc<Server>>,
    JsonBody(request): JsonBody<PairRequest>,
) -> ApiResult<Paired> {
    server.pair(request).map(Json)
}

/// The bare QR payload, for the presenter page and for the Simulator, which
/// reaches this laptop's 127.0.0.1 and fetches it instead of scanning.
async fn app_pairing(_: Local, State(server): State<Arc<Server>>) -> Result<Response, ApiError> {
    let session = blocking(move || server.app_session()).await?;
    Ok((
        [
            (
                header::HeaderName::from_static(EXPIRES_IN),
                header::HeaderValue::from(session.expires_in),
            ),
            (
                header::CACHE_CONTROL,
                header::HeaderValue::from_static("no-store"),
            ),
        ],
        Json(session.qr),
    )
        .into_response())
}

/// The payload and its expiry in one body, for clients that predate
/// `/api/v2/pairing`.
async fn app_session(_: Local, State(server): State<Arc<Server>>) -> ApiResult<AppSession> {
    Ok(Json(blocking(move || server.app_session()).await?))
}

async fn app_pair(
    _: Direct,
    State(server): State<Arc<Server>>,
    JsonBody(request): JsonBody<AppPairRequest>,
) -> ApiResult<Paired> {
    Ok(Json(server.app_pair(request)?))
}

async fn proof_upload(
    State(server): State<Arc<Server>>,
    JsonBody(upload): JsonBody<ProofUpload>,
) -> ApiResult<Accepted> {
    Ok(Json(blocking(move || server.accept_proof(upload)).await?))
}

/// The public parameters the app proves with, streamed, with range requests.
async fn params(
    State(server): State<Arc<Server>>,
    UrlPath(file): UrlPath<String>,
    request: Request,
) -> Result<Response, ApiError> {
    if !PROVER_FILES.contains(&file.as_str()) {
        return Err(ApiError::not_found(format!("no parameter file {file}")));
    }
    let path = server.workspace.location_tool().params().join(&file);
    if !path.exists() {
        return Err(ApiError::not_found(format!(
            "{file} is not there; run `provenance setup`"
        )));
    }
    let mut response = ServeFile::new(path)
        .oneshot(request)
        .await
        .map(|response| response.map(Body::new))
        .context("serving a parameter file")?;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/octet-stream"),
    );
    Ok(response)
}

async fn device(
    State(server): State<Arc<Server>>,
    UrlPath(id): UrlPath<String>,
) -> ApiResult<Value> {
    Ok(Json(json!({ "trusted": server.is_trusted(&id)? })))
}

async fn cell(
    State(server): State<Arc<Server>>,
    JsonBody(request): JsonBody<CellRequest>,
) -> ApiResult<Region> {
    Ok(Json(blocking(move || server.cell(request)).await?))
}

async fn outline(
    State(server): State<Arc<Server>>,
    UrlPath(cell): UrlPath<String>,
) -> ApiResult<Outline> {
    Ok(Json(
        blocking(move || server.workspace.location_tool().outline(&cell)).await?,
    ))
}

async fn upload(
    State(server): State<Arc<Server>>,
    JsonBody(upload): JsonBody<Upload>,
) -> ApiResult<Accepted> {
    Ok(Json(blocking(move || server.accept(upload)).await?))
}

async fn list(_: Local, State(server): State<Arc<Server>>) -> ApiResult<Vec<Status>> {
    Ok(Json(blocking(move || server.list()).await?))
}

async fn status(
    State(server): State<Arc<Server>>,
    UrlPath(id): UrlPath<String>,
) -> ApiResult<Status> {
    Ok(Json(server.status(&id)?))
}

async fn republish(
    _: Local,
    State(server): State<Arc<Server>>,
    UrlPath(id): UrlPath<String>,
) -> ApiResult<Status> {
    Ok(Json(server.start(&id)?))
}

async fn signed(
    State(server): State<Arc<Server>>,
    UrlPath(id): UrlPath<String>,
) -> Result<Response, ApiError> {
    png(&server.dir(&id)?.join("signed.png"))
}

async fn original(
    _: Local,
    State(server): State<Arc<Server>>,
    UrlPath(id): UrlPath<String>,
) -> Result<Response, ApiError> {
    png(&server.dir(&id)?.join("original.png"))
}

async fn verify(State(server): State<Arc<Server>>, RawBody(body): RawBody) -> ApiResult<Report> {
    Ok(Json(blocking(move || server.verify(&body)).await?))
}

fn png(path: &Path) -> Result<Response, ApiError> {
    let bytes = fs::read(path)
        .map_err(|_| ApiError::not_found(format!("{} is not there yet", shown(path))))?;
    Ok((
        [
            (header::CONTENT_TYPE, "image/png"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        bytes,
    )
        .into_response())
}

fn shown(path: &Path) -> String {
    path.file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
}

/// The location tool and the fingerprint take seconds; keep them off the async threads.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    tokio::task::spawn_blocking(work)
        .await
        .context("a request worker panicked")?
}

/// The app's pairing token: single use, short lived, and replaced after
/// [`PAIRING_ATTEMPTS`] refused attempts, so a token seen over a shoulder or
/// guessed at cannot be tried for long.
struct Pairing {
    token: String,
    issued: Instant,
    ttl: Duration,
    /// Refused attempts since the token was last replaced for them.
    failures: u32,
}

impl Pairing {
    fn new(ttl: Duration) -> Self {
        Self {
            token: random_token(),
            issued: Instant::now(),
            ttl,
            failures: 0,
        }
    }

    fn rotate(&mut self) {
        self.token = random_token();
        self.issued = Instant::now();
    }

    fn expired(&self) -> bool {
        self.issued.elapsed() >= self.ttl
    }

    /// The token to show, and how long it has left.
    fn current(&mut self) -> (String, Duration) {
        if self.expired() {
            self.rotate();
        }
        (
            self.token.clone(),
            self.ttl.saturating_sub(self.issued.elapsed()),
        )
    }

    /// Spends the token when it is the current one, unexpired, and
    /// `possession` holds. Anything else counts as a refused attempt.
    fn redeem(&mut self, token: &str, possession: bool) -> Result<()> {
        let matches = equal_in_constant_time(token.as_bytes(), self.token.as_bytes());
        let reason = match (matches, self.expired(), possession) {
            (true, false, true) => {
                self.rotate();
                return Ok(());
            }
            (false, _, _) => {
                "the pairing token is wrong or was used already; scan the QR code again"
            }
            (true, true, _) => "the pairing token expired; scan the QR code again",
            (true, false, false) => "the signature over the token does not match the public key",
        };
        self.failures += 1;
        if self.failures >= PAIRING_ATTEMPTS {
            self.failures = 0;
            self.rotate();
        }
        bail!(reason)
    }
}

/// Wrong capture page codes. After [`PAIRING_ATTEMPTS`] of them every code,
/// the right one too, is refused for a while.
struct Cooldown {
    failures: u32,
    until: Option<Instant>,
    length: Duration,
}

impl Cooldown {
    fn new(length: Duration) -> Self {
        Self {
            failures: 0,
            until: None,
            length,
        }
    }

    /// Whether to look at an attempt, and counts it when `right` is false.
    fn admit(&mut self, right: bool) -> bool {
        if self.until.is_some_and(|until| Instant::now() < until) {
            return false;
        }
        self.until = None;
        match right {
            true => self.failures = 0,
            false => {
                self.failures += 1;
                if self.failures >= PAIRING_ATTEMPTS {
                    self.failures = 0;
                    self.until = Some(Instant::now() + self.length);
                }
            }
        }
        true
    }
}

fn random_token() -> String {
    let mut bytes = [0u8; 16];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

fn equal_in_constant_time(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0, |diff, (x, y)| diff | (x ^ y)) == 0
}

/// The name the app gave, as the presenter and `devices` show it.
fn device_name(name: &str) -> String {
    let name: String = name
        .chars()
        .filter(|c| !c.is_control())
        .take(64)
        .collect::<String>()
        .trim()
        .to_string();
    match name.is_empty() {
        true => "phone".to_string(),
        false => name,
    }
}

/// Every failure is the client's problem to read, as `{"error": "..."}`.
pub struct ApiError {
    status: StatusCode,
    error: anyhow::Error,
}

impl ApiError {
    fn not_found(message: String) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            error: anyhow!(message),
        }
    }
}

impl<E: Into<anyhow::Error>> From<E> for ApiError {
    fn from(error: E) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            error: error.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = json!({ "error": format!("{:#}", self.error) });
        (self.status, Json(body)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_is_spent_once() {
        let mut pairing = Pairing::new(PAIRING_TTL);
        let (token, left) = pairing.current();
        assert_eq!(token.len(), 32);
        assert!(left <= PAIRING_TTL);
        pairing.redeem(&token, true).unwrap();
        let error = pairing.redeem(&token, true).unwrap_err();
        assert!(error.to_string().contains("used already"), "{error}");
        assert_ne!(pairing.current().0, token);
    }

    #[test]
    fn a_token_without_possession_is_refused_and_kept() {
        let mut pairing = Pairing::new(PAIRING_TTL);
        let (token, _) = pairing.current();
        let error = pairing.redeem(&token, false).unwrap_err();
        assert!(error.to_string().contains("signature"), "{error}");
        pairing.redeem(&token, true).unwrap();
    }

    #[test]
    fn an_expired_token_is_refused_and_replaced() {
        let mut pairing = Pairing::new(Duration::ZERO);
        let token = pairing.token.clone();
        let error = pairing.redeem(&token, true).unwrap_err();
        assert!(error.to_string().contains("expired"), "{error}");
        assert_ne!(pairing.current().0, token);
    }

    #[test]
    fn five_refusals_replace_the_token() {
        let mut pairing = Pairing::new(PAIRING_TTL);
        let (token, _) = pairing.current();
        for _ in 1..PAIRING_ATTEMPTS {
            assert!(pairing.redeem("00", true).is_err());
            assert_eq!(pairing.current().0, token);
        }
        assert!(pairing.redeem("00", true).is_err());
        let (replaced, _) = pairing.current();
        assert_ne!(replaced, token);
        assert!(pairing.redeem(&token, true).is_err());
        pairing.redeem(&replaced, true).unwrap();
    }

    #[test]
    fn wrong_codes_start_a_cooldown_that_refuses_the_right_one() {
        let mut attempts = Cooldown::new(Duration::from_secs(60));
        assert!(attempts.admit(false));
        assert!(attempts.admit(true), "one wrong code locked pairing");
        for _ in 0..PAIRING_ATTEMPTS {
            assert!(attempts.admit(false));
        }
        assert!(!attempts.admit(true));
        assert!(!attempts.admit(false));

        let mut attempts = Cooldown::new(Duration::ZERO);
        for _ in 0..PAIRING_ATTEMPTS {
            assert!(attempts.admit(false));
        }
        assert!(attempts.admit(true), "the cooldown never ended");
    }

    #[test]
    fn app_urls_put_given_addresses_first_and_refuse_anything_but_https() {
        let lan = [Ipv4Addr::new(172, 31, 5, 9)];
        assert_eq!(
            app_urls(&lan, 8792, &[]).unwrap(),
            ["https://172.31.5.9:8792"]
        );
        assert_eq!(
            app_urls(
                &lan,
                8792,
                &[
                    "https://3.14.15.92:8792/".to_string(),
                    "https://172.31.5.9:8792".to_string(),
                ]
            )
            .unwrap(),
            ["https://3.14.15.92:8792", "https://172.31.5.9:8792"]
        );
        for bad in [
            "http://3.14.15.92:8792",
            "3.14.15.92:8792",
            "https://",
            "https://:8792",
            "https://a b",
        ] {
            assert!(app_urls(&lan, 8792, &[bad.to_string()]).is_err(), "{bad}");
        }
    }

    #[test]
    fn names_are_cleaned() {
        assert_eq!(device_name("  Pato's iPhone\n"), "Pato's iPhone");
        assert_eq!(device_name("\u{7}"), "phone");
        assert_eq!(device_name(&"x".repeat(100)).len(), 64);
    }
}
