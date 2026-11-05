//! The HTTP API the capture page talks to: pair a device key, look up the
//! cell for a coordinate, and hand in a capture the phone signed.
//!
//! An upload is checked the way a reader would check it before it is stored:
//! the fingerprint must match the pixels, the receipt must be signed by a
//! trusted key, the envelope must match the secrets, and the cell must be the
//! one the location circuit maps the coordinate to.

use std::{path::PathBuf, sync::Arc};

use anyhow::{ensure, Context, Result};
use axum::{
    extract::{DefaultBodyLimit, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use crop_proof::Fingerprint;
use rand::{rngs::OsRng, Rng};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tower_http::services::ServeDir;

use crate::{
    capture::{Capture, Coordinate, Secrets, Source, Summary},
    image,
    location::Region,
    receipt::{parse_public_key, Receipt, TrustedKeys},
    Workspace,
};

/// A PNG of the original plus its JSON is well under this.
const MAX_BODY_BYTES: usize = 8 * 1024 * 1024;

pub struct Server {
    workspace: Workspace,
    captures: PathBuf,
    /// Shown on the laptop and typed on the phone, so only someone at the
    /// laptop can add a trusted key through the tunnel.
    pairing_code: String,
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
    pub accuracy_meters: Option<f64>,
}

#[derive(Serialize, Deserialize)]
pub struct Accepted {
    pub id: String,
    pub dir: PathBuf,
}

impl Server {
    pub fn new(workspace: Workspace, captures: PathBuf) -> Self {
        Self {
            workspace,
            captures,
            pairing_code: format!("{:06}", OsRng.gen_range(0..1_000_000)),
        }
    }

    pub fn pairing_code(&self) -> &str {
        &self.pairing_code
    }

    /// The API, with `web` served for every other path when given.
    pub fn router(self: Arc<Self>, web: Option<PathBuf>) -> Router {
        let api = Router::new()
            .route("/api/pair", post(pair))
            .route("/api/cell", post(cell))
            .route("/api/captures", post(upload))
            .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
            .with_state(self);
        match web {
            Some(dir) => api.fallback_service(ServeDir::new(dir)),
            None => api,
        }
    }

    fn pair(&self, request: PairRequest) -> Result<Paired> {
        ensure!(request.code == self.pairing_code, "wrong pairing code");
        let key = parse_public_key(&request.public_key)?;
        let device = TrustedKeys::add(&self.workspace.trusted_keys_dir(), &key)?;
        Ok(Paired { device })
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
    fn accept(&self, upload: Upload) -> Result<Accepted> {
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
                accuracy_meters: upload.accuracy_meters,
            },
        };
        let id = capture_id(&capture.receipt);
        let dir = self.captures.join(&id);
        ensure!(!dir.exists(), "capture {id} already exists");
        capture.write(&dir)?;
        Ok(Accepted { id, dir })
    }
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

type ApiResult<T> = std::result::Result<Json<T>, ApiError>;

async fn pair(
    State(server): State<Arc<Server>>,
    Json(request): Json<PairRequest>,
) -> ApiResult<Paired> {
    Ok(Json(server.pair(request)?))
}

async fn cell(
    State(server): State<Arc<Server>>,
    Json(request): Json<CellRequest>,
) -> ApiResult<Region> {
    Ok(Json(blocking(move || server.cell(request)).await?))
}

async fn upload(
    State(server): State<Arc<Server>>,
    Json(upload): Json<Upload>,
) -> ApiResult<Accepted> {
    Ok(Json(blocking(move || server.accept(upload)).await?))
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
pub struct ApiError(anyhow::Error);

impl<E: Into<anyhow::Error>> From<E> for ApiError {
    fn from(error: E) -> Self {
        Self(error.into())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = json!({ "error": format!("{:#}", self.0) });
        (StatusCode::BAD_REQUEST, Json(body)).into_response()
    }
}
