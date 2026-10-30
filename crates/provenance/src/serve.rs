//! The HTTP API the capture page and the presenter page talk to: pair a
//! device key, look up the cell for a coordinate, hand in a capture the phone
//! signed, follow it while the laptop publishes it, and verify any file.
//!
//! An upload is checked the way a reader would check it before it is stored:
//! the fingerprint must match the pixels, the receipt must be signed by a
//! trusted key, the envelope must match the secrets, and the cell must be the
//! one the location circuit maps the coordinate to.
//!
//! The server runs behind a tunnel, so every route is reachable from the
//! internet unless it asks for [`Local`]. Nothing private (the original, the
//! secrets) is served to a request that came through the tunnel.

use std::{
    collections::HashMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    thread,
};

use anyhow::{anyhow, ensure, Context, Result};
use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, FromRequestParts, Path as UrlPath, State},
    http::{header, request::Parts, HeaderMap, StatusCode},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use crop_proof::{Fingerprint, Rect};
use rand::{rngs::OsRng, Rng};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tower_http::{services::ServeDir, set_header::SetResponseHeaderLayer};

use crate::{
    capture::{Capture, Coordinate, Secrets, Source, Summary},
    image,
    location::{Outline, Region},
    publish,
    receipt::{parse_public_key, Receipt, TrustedKeys},
    verify::Verdict,
    Workspace,
};

/// A PNG of the original plus its JSON is well under this.
const MAX_BODY_BYTES: usize = 8 * 1024 * 1024;

/// How many captures the presenter page lists, newest first.
const LISTED: usize = 50;

type Listener = Box<dyn Fn(&Status) + Send + Sync>;

pub struct Server {
    workspace: Workspace,
    captures: PathBuf,
    /// Shown on the laptop and typed on the phone, so only someone at the
    /// laptop can add a trusted key through the tunnel.
    pairing_code: String,
    /// The tunnel's address, for the QR code on the presenter page.
    public_url: Option<String>,
    /// Whether an accepted upload is published right away.
    publishes: bool,
    /// Captures this process has published or is publishing, by id.
    jobs: Mutex<HashMap<String, Status>>,
    /// One proof at a time: the crop proof already uses every core.
    prover: Mutex<()>,
    on_change: Option<Listener>,
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
    /// Base64 PNG of the 1024x512 original the device fingerprinted.
    pub original_png: String,
    pub secrets: Secrets,
    pub receipt: Receipt,
    pub cell: String,
    pub resolution: u8,
    /// The rectangle the photographer chose to publish.
    pub crop: Rect,
    pub accuracy_meters: Option<f64>,
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
    pub captured_at: String,
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
        Self {
            workspace,
            captures,
            pairing_code: format!("{:06}", OsRng.gen_range(0..1_000_000)),
            public_url: None,
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

    fn pair(&self, request: PairRequest) -> Result<Paired> {
        ensure!(request.code == self.pairing_code, "wrong pairing code");
        let key = parse_public_key(&request.public_key)?;
        let device = TrustedKeys::add(&self.workspace.trusted_keys_dir(), &key)?;
        Ok(Paired { device })
    }

    fn is_trusted(&self, device: &str) -> Result<bool> {
        ensure!(
            !device.is_empty() && device.chars().all(|c| c.is_ascii_hexdigit()),
            "a device id is hex"
        );
        Ok(self
            .workspace
            .trusted_keys_dir()
            .join(format!("{device}.pem"))
            .exists())
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

    /// Checks the upload as a reader would and stores it as a capture directory.
    fn accept(self: &Arc<Self>, upload: Upload) -> Result<Accepted> {
        let png = BASE64
            .decode(&upload.original_png)
            .context("original_png is not base64")?;
        let original = image::decode_png(&png)?;
        ensure!(
            Fingerprint::of(&original)? == upload.receipt.fingerprint,
            "the fingerprint does not match the pixels"
        );
        upload
            .receipt
            .verify(&TrustedKeys::load(&self.workspace.trusted_keys_dir())?)?;
        let tool = self.workspace.location_tool();
        ensure!(
            tool.commit(&upload.secrets)? == upload.receipt.envelope,
            "the envelope does not match the secrets"
        );
        upload.crop.check()?;
        let region = tool.cell(upload.secrets.coordinate(), upload.resolution)?;
        ensure!(
            region.cell == upload.cell,
            "the coordinate is in cell {} at resolution {}, not {}",
            region.cell,
            upload.resolution,
            upload.cell
        );

        let capture = Capture {
            original,
            secrets: upload.secrets,
            receipt: upload.receipt,
            summary: Summary {
                source: Source::Phone,
                cell: upload.cell,
                resolution: upload.resolution,
                crop: upload.crop,
                accuracy_meters: upload.accuracy_meters,
            },
        };
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
            device: receipt.device,
            captured_at: receipt.captured_at,
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

/// Serves `router` on localhost until the process ends.
pub fn run(router: Router, port: u16) -> Result<()> {
    tokio::runtime::Runtime::new()?.block_on(async {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
            .await
            .with_context(|| format!("binding port {port}"))?;
        axum::serve(listener, router).await.context("serving")
    })
}

/// A request made on the laptop itself. One that came through the tunnel
/// names the tunnel's host and carries X-Forwarded-For, even though it
/// arrives from 127.0.0.1 like a local one.
pub struct Local;

impl<S: Send + Sync> FromRequestParts<S> for Local {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        match is_local(&parts.headers) {
            true => Ok(Local),
            false => Err(ApiError {
                status: StatusCode::FORBIDDEN,
                error: anyhow!("only the laptop running provenance serve can open this"),
            }),
        }
    }
}

fn is_local(headers: &HeaderMap) -> bool {
    let host = headers
        .get(header::HOST)
        .and_then(|host| host.to_str().ok())
        .unwrap_or_default();
    let name = host.rsplit_once(':').map_or(host, |(name, _)| name);
    !headers.contains_key("x-forwarded-for") && matches!(name, "localhost" | "127.0.0.1")
}

type ApiResult<T> = std::result::Result<Json<T>, ApiError>;

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
    Json(request): Json<PairRequest>,
) -> ApiResult<Paired> {
    Ok(Json(server.pair(request)?))
}

async fn device(
    State(server): State<Arc<Server>>,
    UrlPath(id): UrlPath<String>,
) -> ApiResult<Value> {
    Ok(Json(json!({ "trusted": server.is_trusted(&id)? })))
}

async fn cell(
    State(server): State<Arc<Server>>,
    Json(request): Json<CellRequest>,
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
    Json(upload): Json<Upload>,
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

async fn verify(State(server): State<Arc<Server>>, body: Bytes) -> ApiResult<Report> {
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
