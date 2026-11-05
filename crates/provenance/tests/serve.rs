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
        crop: capture.summary.crop,
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
        crop_proof::Rect {
            x: 200,
            y: 100,
            width: 640,
            height: 360,
        },
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

    let mut wrong_crop = genuine.clone();
    wrong_crop["crop"]["width"] = 900.into();
    let (_, body) = post(&router, "/api/captures", &wrong_crop).await;
    assert!(error_of(&body).contains("does not fit"), "{body}");

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
        crop_proof::Rect::LEFT_HALF,
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

async fn get(router: &Router, path: &str, local: bool) -> (StatusCode, Vec<u8>) {
    let mut request = Request::get(path);
    if local {
        request = request.header(header::HOST, "127.0.0.1:8791");
    }
    let response = router
        .clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, bytes.to_vec())
}

async fn get_json(router: &Router, path: &str, local: bool) -> (StatusCode, Value) {
    let (status, bytes) = get(router, path, local).await;
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs `provenance setup`; run with --ignored"]
async fn publishes_an_upload_and_serves_only_public_files_through_the_tunnel() {
    let home = scratch_home(&real_home());
    let workspace = Workspace::new(home.path());
    let captures = tempfile::tempdir().unwrap();
    let server = Arc::new(
        Server::new(Workspace::new(home.path()), captures.path().to_path_buf())
            .public_url(Some("https://example.ngrok.app/".to_string()))
            .publishing(),
    );
    let router = Arc::clone(&server).router(None);

    let capture = Capture::simulate(
        &workspace.location_tool(),
        &workspace.device_key().unwrap(),
        image::load_photo(&workspace.sample_photo()).unwrap(),
        DEMO_PLACE.coordinate,
        DEMO_PLACE.cell,
        crop_proof::Rect {
            x: 100,
            y: 50,
            width: 300,
            height: 200,
        },
    )
    .unwrap();
    let (status, body) = post(
        &router,
        "/api/captures",
        &serde_json::to_value(upload_of(&capture)).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let id = body["id"].as_str().unwrap().to_string();

    let path = format!("/api/captures/{id}");
    let mut body = Value::Null;
    // Up to 10 minutes: CI runners take over 2 minutes to prove the crop.
    for _ in 0..1200 {
        (_, body) = get_json(&router, &path, false).await;
        if !["queued", "publishing", "verifying"].contains(&body["phase"].as_str().unwrap()) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    assert_eq!(body["phase"], "accepted", "{body}");
    assert_eq!(body["claim"]["cell"], DEMO_PLACE.cell);
    let events: Vec<&str> = body["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|event| event["event"].as_str().unwrap())
        .collect();
    assert_eq!(events.iter().filter(|e| **e == "finished").count(), 4);
    assert_eq!(events.iter().filter(|e| **e == "passed").count(), 4);
    assert!(body.get("secrets").is_none() && !body.to_string().contains(&capture.secrets.salt));

    let (status, signed) = get(&router, &format!("{path}/signed.png"), false).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = get(&router, &format!("{path}/original.png"), false).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "the original went through the tunnel"
    );
    let (status, _) = get(&router, &format!("{path}/original.png"), true).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = get(&router, "/api/session", false).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "the pairing code went through the tunnel"
    );
    let (status, session) = get_json(&router, "/api/session", true).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        session["pair_url"],
        format!("https://example.ngrok.app/#pair={}", server.pairing_code())
    );
    let (status, listed) = get_json(&router, "/api/captures", true).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed[0]["id"], id.as_str());
    let (status, _) = get(&router, "/api/captures/..%2F..%2Fetc", false).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let report = verify(&router, signed).await;
    assert!(report["verdict"]["rejected"].is_null(), "{report}");
    assert_eq!(report["verdict"]["claim"]["crop"]["width"], 300);
    let report = verify(&router, b"not a png".to_vec()).await;
    assert_eq!(
        report["verdict"]["rejected"]["check"], "manifest",
        "{report}"
    );
}

async fn verify(router: &Router, png: Vec<u8>) -> Value {
    let request = Request::post("/api/verify")
        .header(header::CONTENT_TYPE, "image/png")
        .body(Body::from(png))
        .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap()
}
