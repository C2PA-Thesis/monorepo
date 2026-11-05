//! The capture API against a real workspace. Needs `provenance setup` first,
//! so it runs with `--ignored` after setup, locally and in CI.

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::Arc,
};

use axum::{
    body::Body,
    http::{header, Request, StatusCode},
    Router,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use http_body_util::BodyExt;
use provenance::{
    capture::Capture,
    demo::DEMO_PLACE,
    image,
    receipt::DeviceKey,
    serve::{Accepted, Server, Upload},
    Workspace,
};
use serde_json::{json, Value};
use tower::ServiceExt;

/// A throwaway home that shares the built tool, parameters and samples with
/// the real one, so pairing cannot add keys to it.
fn scratch_home(real: &Path) -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    for shared in ["bin", "params", "c2pa", "sample.jpg"] {
        std::os::unix::fs::symlink(real.join(shared), home.path().join(shared)).unwrap();
    }
    fs::copy(real.join("device.pem"), home.path().join("device.pem")).unwrap();
    fs::create_dir(home.path().join("trusted")).unwrap();
    for entry in fs::read_dir(real.join("trusted")).unwrap() {
        let entry = entry.unwrap();
        fs::copy(
            entry.path(),
            home.path().join("trusted").join(entry.file_name()),
        )
        .unwrap();
    }
    home
}

fn real_home() -> PathBuf {
    std::env::var_os("PROVENANCE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.provenance"))
}

async fn post(router: &Router, path: &str, body: &Value) -> (StatusCode, Value) {
    let request = Request::post(path)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap())
}

fn upload_of(capture: &Capture) -> Upload {
    Upload {
        original_png: BASE64.encode(image::encode_png(&capture.original).unwrap()),
        secrets: capture.secrets.clone(),
        receipt: capture.receipt.clone(),
        cell: capture.summary.cell.clone(),
        resolution: capture.summary.resolution,
        accuracy_meters: Some(5.0),
    }
}

fn error_of(body: &Value) -> &str {
    body["error"].as_str().unwrap_or("")
}

#[tokio::test]
#[ignore = "needs `provenance setup`; run with --ignored"]
async fn accepts_a_signed_capture_and_rejects_tampered_ones() {
    let home = scratch_home(&real_home());
    let workspace = Workspace::new(home.path());
    let tool = workspace.location_tool();
    let captures = tempfile::tempdir().unwrap();
    let server = Arc::new(Server::new(
        Workspace::new(home.path()),
        captures.path().to_path_buf(),
    ));
    let code = server.pairing_code().to_string();
    let router = server.router(None);

    let (status, body) = post(
        &router,
        "/api/cell",
        &json!({"latitude": DEMO_PLACE.coordinate.latitude, "longitude": DEMO_PLACE.coordinate.longitude, "resolution": 7}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["cell"], DEMO_PLACE.cell);

    let capture = Capture::simulate(
        &tool,
        &workspace.device_key().unwrap(),
        image::load_photo(&workspace.sample_photo()).unwrap(),
        DEMO_PLACE.coordinate,
        DEMO_PLACE.cell,
    )
    .unwrap();
    let genuine = serde_json::to_value(upload_of(&capture)).unwrap();

    let (status, body) = post(&router, "/api/captures", &genuine).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let accepted: Accepted = serde_json::from_value(body).unwrap();
    let stored = Capture::read(&accepted.dir).unwrap();
    assert_eq!(stored.receipt, capture.receipt);
    assert_eq!(stored.original, capture.original);
    let mode = fs::metadata(accepted.dir.join("secrets.json"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);

    let (status, body) = post(&router, "/api/captures", &genuine).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(error_of(&body).contains("already exists"), "{body}");

    let mut wrong_pixels = genuine.clone();
    let mut other = capture.original.channels().clone();
    other[0][0] ^= 1;
    let other = crop_proof::RgbImage::new(capture.original.size(), other).unwrap();
    wrong_pixels["original_png"] = BASE64.encode(image::encode_png(&other).unwrap()).into();
    let (_, body) = post(&router, "/api/captures", &wrong_pixels).await;
    assert!(
        error_of(&body).contains("fingerprint does not match"),
        "{body}"
    );

    let mut forged = genuine.clone();
    forged["receipt"]["captured_at"] = "2020-01-01T00:00:00Z".into();
    let (_, body) = post(&router, "/api/captures", &forged).await;
    assert!(
        error_of(&body).contains("signature does not match"),
        "{body}"
    );

    let mut wrong_secrets = genuine.clone();
    wrong_secrets["secrets"]["salt"] = format!("0x{}", "1".repeat(64)).into();
    let (_, body) = post(&router, "/api/captures", &wrong_secrets).await;
    assert!(
        error_of(&body).contains("envelope does not match"),
        "{body}"
    );

    let mut wrong_cell = genuine.clone();
    wrong_cell["cell"] = "87c2e3021ffffff".into();
    let (_, body) = post(&router, "/api/captures", &wrong_cell).await;
    assert!(error_of(&body).contains("not 87c2e3021ffffff"), "{body}");

    let phone = DeviceKey::generate();
    let unknown = Capture::simulate(
        &tool,
        &phone,
        capture.original.clone(),
        DEMO_PLACE.coordinate,
        DEMO_PLACE.cell,
    )
    .unwrap();
    let from_phone = serde_json::to_value(upload_of(&unknown)).unwrap();
    let (_, body) = post(&router, "/api/captures", &from_phone).await;
    assert!(error_of(&body).contains("not trusted"), "{body}");

    let public_key = p256::pkcs8::EncodePublicKey::to_public_key_pem(
        &phone.public(),
        p256::pkcs8::LineEnding::LF,
    )
    .unwrap();
    let (status, body) = post(
        &router,
        "/api/pair",
        &json!({"code": "000000", "public_key": public_key}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let (status, body) = post(
        &router,
        "/api/pair",
        &json!({"code": code, "public_key": public_key}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["device"], phone.id().unwrap());
    let (status, body) = post(&router, "/api/captures", &from_phone).await;
    assert_eq!(status, StatusCode::OK, "{body}");
}
