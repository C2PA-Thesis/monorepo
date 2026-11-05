//! The capture API against a real workspace. Needs `provenance setup` first,
//! so it runs with `--ignored` after setup, locally and in CI.

use std::{
    fs,
    net::SocketAddr,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::Duration,
};

use axum::{
    body::Body,
    extract::connect_info::MockConnectInfo,
    http::{header, Request, StatusCode},
    Router,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use http_body_util::BodyExt;
use p256::{
    ecdsa::{signature::Signer, Signature, SigningKey},
    pkcs8::{EncodePrivateKey, EncodePublicKey, LineEnding},
};
use provenance::{
    capture::{Capture, LocationSource, Witness},
    demo::{DEMO_PLACE, OTHER_PLACE},
    image,
    location::LocationProof,
    manifest, publish,
    receipt::DeviceKey,
    serve::{self, Accepted, AppSession, Lan, PhoneProof, ProofUpload, Server, Upload},
    verify::Check,
    Workspace,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
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
        secrets: capture.secrets().unwrap().clone(),
        receipt: capture.receipt.clone(),
        cell: capture.summary.cell.clone(),
        resolution: capture.summary.resolution,
        crop: capture.summary.crop,
        turn: capture.summary.turn,
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

    // A home set up before sidecars existed: the laptop's key has none.
    let laptop = workspace.device_key().unwrap().id().unwrap();
    let _ = fs::remove_file(home.path().join(format!("trusted/{laptop}.json")));
    let (status, body) = post(&router, "/api/captures", &genuine).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let accepted: Accepted = serde_json::from_value(body).unwrap();
    let (_, body) = get_json(&router, &format!("/api/captures/{}", accepted.id), false).await;
    assert_eq!(body["device_kind"], "laptop", "{body}");
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

    // Hints from the upload reach location-proof, which refuses another
    // coordinate's.
    let mut with_hints = genuine.clone();
    with_hints["secrets"]["hints"] = json!(hints_with_cli(
        &location_proof_cli(home.path()),
        DEMO_PLACE.coordinate
    ));
    assert_eq!(
        tool.commit(&serde_json::from_value(with_hints["secrets"].clone()).unwrap())
            .unwrap(),
        capture.receipt.envelope
    );
    let mut wrong_hints = genuine.clone();
    wrong_hints["secrets"]["hints"] = json!(hints_with_cli(
        &location_proof_cli(home.path()),
        OTHER_PLACE.coordinate
    ));
    let (_, body) = post(&router, "/api/captures", &wrong_hints).await;
    assert!(
        error_of(&body).contains("hints belong to another coordinate"),
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
        crop_proof::Rect::left_half(capture.original.size()),
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

    let mut capture = Capture::simulate(
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
    // Published turned, so the server proves a turned crop end to end.
    capture.summary.turn = crop_proof::Turn::new(1).unwrap();
    let (status, body) = post(
        &router,
        "/api/captures",
        &serde_json::to_value(upload_of(&capture)).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let id = body["id"].as_str().unwrap().to_string();

    let path = format!("/api/captures/{id}");
    let body = published(&router, &id).await;
    assert_eq!(body["phase"], "accepted", "{body}");
    assert_eq!(body["claim"]["cell"], DEMO_PLACE.cell);
    assert_eq!(body["claim"]["turn"], 1, "{body}");
    let events: Vec<&str> = body["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|event| event["event"].as_str().unwrap())
        .collect();
    assert_eq!(events.iter().filter(|e| **e == "finished").count(), 4);
    assert_eq!(events.iter().filter(|e| **e == "passed").count(), 4);
    let salt = &capture.secrets().unwrap().salt;
    assert!(body.get("secrets").is_none() && !body.to_string().contains(salt));

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

    let oversized = Request::post("/api/verify")
        .header(header::CONTENT_TYPE, "image/png")
        .body(Body::from(vec![0u8; 8 * 1024 * 1024 + 1]))
        .unwrap();
    let response = router.clone().oneshot(oversized).await.unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
    let body: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert!(error_of(&body).contains("length limit"), "{body}");
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

/// Polls a capture until publishing ends, and returns its last status.
/// The crop proof takes minutes on a busy machine.
async fn published(router: &Router, id: &str) -> Value {
    let path = format!("/api/captures/{id}");
    let mut body = Value::Null;
    for _ in 0..1200 {
        (_, body) = get_json(router, &path, false).await;
        if !["queued", "publishing", "verifying"].contains(&body["phase"].as_str().unwrap()) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    body
}

/// A stand-in for the app's key, as a device key and as the raw signer the
/// pairing proof needs.
fn app_key(dir: &Path) -> (SigningKey, DeviceKey) {
    let key = SigningKey::random(&mut rand::rngs::OsRng);
    let path = dir.join("app.pem");
    fs::write(&path, key.to_pkcs8_pem(LineEnding::LF).unwrap().as_bytes()).unwrap();
    (key.clone(), DeviceKey::load(&path).unwrap())
}

fn pair_body(key: &SigningKey, token: &str, signed: &str, kind: &str) -> Value {
    let signature: Signature = key.sign(signed.as_bytes());
    json!({
        "token": token,
        "public_key": key.verifying_key().to_public_key_pem(LineEnding::LF).unwrap(),
        "signature": BASE64.encode(signature.to_der()),
        "kind": kind,
        "name": "Test iPhone",
    })
}

fn test_lan() -> Lan {
    Lan {
        urls: vec!["https://192.0.2.10:8792".to_string()],
        pin: "ab".repeat(32),
    }
}

async fn app_session(router: &Router) -> AppSession {
    let (status, body) = get_json(router, "/api/v2/session", true).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    serde_json::from_value(body).unwrap()
}

/// `GET /api/v2/pairing` from the laptop: the raw body, which the QR code
/// holds as is, and the seconds its token has left.
async fn app_pairing(router: &Router) -> (String, u64) {
    let request = Request::get("/api/v2/pairing")
        .header(header::HOST, "127.0.0.1:8791")
        .body(Body::empty())
        .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let expires_in = response.headers()[serve::EXPIRES_IN]
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (String::from_utf8(bytes.to_vec()).unwrap(), expires_in)
}

fn location_proof_cli(home: &Path) -> PathBuf {
    home.join("bin/location-proof")
}

#[tokio::test]
#[ignore = "needs `provenance setup`; run with --ignored"]
async fn pairs_the_app_with_a_single_use_token_and_proof_of_possession() {
    let home = scratch_home(&real_home());
    let captures = tempfile::tempdir().unwrap();
    let server = Arc::new(
        Server::new(Workspace::new(home.path()), captures.path().to_path_buf()).lan(test_lan()),
    );
    let router = Arc::clone(&server).router(None);

    let lan_peer = router.clone().layer(MockConnectInfo(SocketAddr::from((
        [192, 168, 1, 20],
        50000,
    ))));
    let laptop = router
        .clone()
        .layer(MockConnectInfo(SocketAddr::from(([127, 0, 0, 1], 50000))));
    for path in ["/api/v2/pairing", "/api/v2/session"] {
        let (status, _) = get(&router, path, false).await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "{path}: the token went through the tunnel"
        );
        let (status, _) = get(&lan_peer, path, true).await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "{path}: a phone on the network claimed to be the laptop"
        );
        let (status, _) = get(&laptop, path, true).await;
        assert_eq!(status, StatusCode::OK, "{path}");
    }

    // The bare payload of the wire format, in its key order, and nothing else.
    let (payload, expires_in) = app_pairing(&router).await;
    assert!(expires_in > 290 && expires_in <= 300, "{expires_in}");
    assert!(
        payload.starts_with(r#"{"v":1,"urls":["https://192.0.2.10:8792"],"pin":""#),
        "{payload}"
    );
    let qr: Value = serde_json::from_str(&payload).unwrap();
    let mut keys: Vec<&String> = qr.as_object().unwrap().keys().collect();
    keys.sort();
    assert_eq!(keys, ["params", "pin", "token", "urls", "v"]);
    let order: Vec<usize> = [r#""proving.key""#, r#""circuit.r1cs""#]
        .iter()
        .map(|key| payload.find(key).unwrap())
        .collect();
    assert!(order[0] < order[1], "{payload}");
    for digest in qr["params"].as_object().unwrap().values() {
        assert_eq!(digest.as_str().unwrap().len(), 64);
    }
    let session = app_session(&router).await;
    assert_eq!(serde_json::to_string(&session.qr).unwrap(), payload);

    let qr = serde_json::to_value(&session.qr).unwrap();
    assert_eq!(qr["v"], 1);
    assert_eq!(qr["urls"], json!(["https://192.0.2.10:8792"]));
    assert_eq!(qr["pin"], "ab".repeat(32));
    assert_eq!(session.qr.token.len(), 32);
    assert!(session.expires_in > 290 && session.expires_in <= 300);
    let keys: Vec<&String> = qr["params"].as_object().unwrap().keys().collect();
    assert_eq!(
        keys,
        ["proving.key", "circuit.r1cs"].iter().collect::<Vec<_>>()
    );

    let dir = tempfile::tempdir().unwrap();
    let (key, device) = app_key(dir.path());
    let token = session.qr.token.clone();
    assert_eq!(qr["token"], token.as_str());

    let (status, body) = post(
        &router,
        "/api/v2/pair",
        &pair_body(&key, &token, "other", "software"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        error_of(&body).contains("signature over the token"),
        "{body}"
    );
    let (status, body) = post(
        &router,
        "/api/v2/pair",
        &pair_body(&key, &token, &token, "browser"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(error_of(&body).contains("kind must be"), "{body}");
    assert_eq!(
        app_session(&router).await.qr.token,
        token,
        "two refusals replaced the token"
    );

    let (status, body) = post(
        &router,
        "/api/v2/pair",
        &pair_body(&key, &token, &token, "secure-enclave"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let id = device.id().unwrap();
    assert_eq!(body["device"], id.as_str());
    let sidecar: Value =
        serde_json::from_slice(&fs::read(home.path().join(format!("trusted/{id}.json"))).unwrap())
            .unwrap();
    assert_eq!(sidecar["kind"], "secure-enclave");
    assert_eq!(sidecar["name"], "Test iPhone");
    assert!(sidecar["paired_at"].as_str().unwrap().ends_with('Z'));
    let (_, body) = get_json(&router, &format!("/api/devices/{id}"), false).await;
    assert_eq!(body["trusted"], true);

    let (status, body) = post(
        &router,
        "/api/pair",
        &json!({
            "code": server.pairing_code(),
            "public_key": key.verifying_key().to_public_key_pem(LineEnding::LF).unwrap(),
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["device"], id.as_str());
    let relabeled: Value =
        serde_json::from_slice(&fs::read(home.path().join(format!("trusted/{id}.json"))).unwrap())
            .unwrap();
    assert_eq!(
        relabeled, sidecar,
        "capture page pairing relabeled an app key"
    );

    let (status, body) = post(
        &router,
        "/api/v2/pair",
        &pair_body(&key, &token, &token, "secure-enclave"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(error_of(&body).contains("used already"), "{body}");
    assert_ne!(
        app_session(&router).await.qr.token,
        token,
        "the token was not replaced once used"
    );
    let (payload, _) = app_pairing(&router).await;
    assert!(
        !payload.contains(&token),
        "the pairing route showed a used token"
    );
}

#[tokio::test]
#[ignore = "needs `provenance setup`; run with --ignored"]
async fn expires_tokens_and_replaces_one_after_five_refusals() {
    let home = scratch_home(&real_home());
    let captures = tempfile::tempdir().unwrap();
    let router = Arc::new(
        Server::new(Workspace::new(home.path()), captures.path().to_path_buf())
            .lan(test_lan())
            .pairing_ttl(Duration::from_millis(400)),
    )
    .router(None);
    let dir = tempfile::tempdir().unwrap();
    let (key, _) = app_key(dir.path());

    let token = app_session(&router).await.qr.token;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let (status, body) = post(
        &router,
        "/api/v2/pair",
        &pair_body(&key, &token, &token, "software"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(error_of(&body).contains("expired"), "{body}");
    assert_ne!(
        app_session(&router).await.qr.token,
        token,
        "an expired token was shown again"
    );

    let router = Arc::new(
        Server::new(Workspace::new(home.path()), captures.path().to_path_buf()).lan(test_lan()),
    )
    .router(None);
    let token = app_session(&router).await.qr.token;
    let wrong = "0".repeat(32);

    // Attempts from the internet are refused before they count, so they
    // cannot replace the token the presenter shows.
    for _ in 0..serve::PAIRING_ATTEMPTS {
        let request = Request::post("/api/v2/pair")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::HOST, "example.ngrok.app")
            .header("x-forwarded-for", "203.0.113.9")
            .body(Body::from(
                pair_body(&key, &wrong, &wrong, "software").to_string(),
            ))
            .unwrap();
        let response = router.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
    assert_eq!(
        app_session(&router).await.qr.token,
        token,
        "attempts through the tunnel replaced the token"
    );

    // A body that does not parse is still answered as {"error": ...}.
    let (status, body) = post(
        &router,
        "/api/v2/pair",
        &pair_body(&key, &wrong, &wrong, "bogus"),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(error_of(&body).contains("unknown variant"), "{body}");
    assert_eq!(app_session(&router).await.qr.token, token);
    for attempt in 1..=serve::PAIRING_ATTEMPTS {
        let (status, body) = post(
            &router,
            "/api/v2/pair",
            &pair_body(&key, &wrong, &wrong, "software"),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(error_of(&body).contains("wrong"), "{body}");
        let shown = app_session(&router).await.qr.token;
        match attempt < serve::PAIRING_ATTEMPTS {
            true => assert_eq!(shown, token, "replaced after {attempt} refusals"),
            false => assert_ne!(shown, token, "kept after {attempt} refusals"),
        }
    }
    let (status, _) = post(
        &router,
        "/api/v2/pair",
        &pair_body(&key, &token, &token, "software"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "the replaced token still paired"
    );
}

#[tokio::test]
#[ignore = "needs `provenance setup`; run with --ignored"]
async fn serves_the_parameters_the_qr_code_names() {
    let home = scratch_home(&real_home());
    let captures = tempfile::tempdir().unwrap();
    let router = Arc::new(
        Server::new(Workspace::new(home.path()), captures.path().to_path_buf()).lan(test_lan()),
    )
    .router(None);
    let (payload, _) = app_pairing(&router).await;
    let qr: Value = serde_json::from_str(&payload).unwrap();
    for file in ["proving.key", "circuit.r1cs"] {
        // Through the tunnel too: the parameters are public.
        let (status, bytes) = get(&router, &format!("/api/params/location/{file}"), false).await;
        assert_eq!(status, StatusCode::OK, "{file}");
        assert_eq!(
            hex::encode(Sha256::digest(&bytes)),
            qr["params"][file].as_str().unwrap(),
            "{file}"
        );
        let on_disk = fs::read(home.path().join("params/location").join(file)).unwrap();
        assert_eq!(bytes, on_disk, "{file}");
    }
    let request = Request::get("/api/params/location/proving.key")
        .header(header::RANGE, "bytes=0-9")
        .body(Body::empty())
        .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .len(),
        10
    );
    for refused in ["verifying.key", "..%2F..%2Fdevice.pem", "version"] {
        let (status, _) = get(&router, &format!("/api/params/location/{refused}"), false).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{refused}");
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs `provenance setup`; run with --ignored"]
async fn publishes_a_location_proof_made_on_the_phone_without_its_secrets() {
    let home = scratch_home(&real_home());
    let workspace = Workspace::new(home.path());
    let tool = workspace.location_tool();
    let captures = tempfile::tempdir().unwrap();
    let router = Arc::new(
        Server::new(Workspace::new(home.path()), captures.path().to_path_buf())
            .lan(test_lan())
            .publishing(),
    )
    .router(None);

    let dir = tempfile::tempdir().unwrap();
    let (key, device) = app_key(dir.path());
    let token = app_session(&router).await.qr.token;
    let (status, body) = post(
        &router,
        "/api/v2/pair",
        &pair_body(&key, &token, &token, "software"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // The phone's side: seal the coordinate, sign the receipt, prove the cell.
    // The proof comes from `location-proof prove`, the same code the app links.
    let capture = Capture::simulate(
        &tool,
        &device,
        image::load_photo(&workspace.sample_photo()).unwrap(),
        DEMO_PLACE.coordinate,
        DEMO_PLACE.cell,
        crop_proof::Rect {
            x: 300,
            y: 100,
            width: 400,
            height: 300,
        },
    )
    .unwrap();
    // The app always sends the hints it computed; the envelope is the same
    // as the one location-proof computes without them.
    let mut secrets = capture.secrets().unwrap().clone();
    secrets.hints = Some(hints_with_cli(
        &location_proof_cli(home.path()),
        DEMO_PLACE.coordinate,
    ));
    assert_eq!(tool.commit(&secrets).unwrap(), capture.receipt.envelope);
    let proof = prove_with_cli(
        &location_proof_cli(home.path()),
        &home.path().join("params/location"),
        &secrets,
        DEMO_PLACE.cell,
    );
    assert_eq!(proof.envelope, capture.receipt.envelope);
    let upload = ProofUpload {
        original_png: BASE64.encode(image::encode_png(&capture.original).unwrap()),
        receipt: capture.receipt.clone(),
        cell: DEMO_PLACE.cell.to_string(),
        resolution: 7,
        crop: capture.summary.crop,
        turn: capture.summary.turn,
        accuracy_meters: Some(4.0),
        location_proof: PhoneProof::Proof(proof.proof.clone()),
        location: Some(LocationSource {
            simulated: true,
            accessory: false,
        }),
    };
    let genuine = serde_json::to_value(&upload).unwrap();
    assert!(!genuine.to_string().contains(&secrets.salt));

    let other = Capture::simulate(
        &tool,
        &device,
        capture.original.clone(),
        OTHER_PLACE.coordinate,
        OTHER_PLACE.cell,
        crop_proof::Rect::left_half(capture.original.size()),
    )
    .unwrap();
    let other_proof = tool
        .prove(other.secrets().unwrap(), OTHER_PLACE.cell)
        .unwrap();
    let mut wrong_proof = genuine.clone();
    wrong_proof["location_proof"] = other_proof.proof.clone().into();
    let (_, body) = post(&router, "/api/v2/captures", &wrong_proof).await;
    assert!(
        error_of(&body).contains("location proof does not verify"),
        "{body}"
    );
    let mut wrong_cell = genuine.clone();
    wrong_cell["cell"] = OTHER_PLACE.cell.into();
    let (_, body) = post(&router, "/api/v2/captures", &wrong_cell).await;
    assert!(
        error_of(&body).contains("location proof does not verify"),
        "{body}"
    );
    let mut wrong_envelope = genuine.clone();
    wrong_envelope["location_proof"] =
        json!({"envelope": other_proof.envelope, "proof": proof.proof});
    let (_, body) = post(&router, "/api/v2/captures", &wrong_envelope).await;
    assert!(error_of(&body).contains("not the receipt's"), "{body}");
    let mut wrong_resolution = genuine.clone();
    wrong_resolution["resolution"] = 8.into();
    let (_, body) = post(&router, "/api/v2/captures", &wrong_resolution).await;
    assert!(error_of(&body).contains("at resolution 7, not 8"), "{body}");
    let mut with_secrets = genuine.clone();
    with_secrets["secrets"] = serde_json::to_value(&secrets).unwrap();
    let request = Request::post("/api/v2/captures")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(with_secrets.to_string()))
        .unwrap();
    let status = router.clone().oneshot(request).await.unwrap().status();
    assert!(status.is_client_error(), "an upload with secrets was taken");

    let mut as_returned = genuine.clone();
    as_returned["location_proof"] = json!({"envelope": proof.envelope, "proof": proof.proof});
    let (status, body) = post(&router, "/api/v2/captures", &as_returned).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let accepted: Accepted = serde_json::from_value(body).unwrap();
    assert!(!accepted.dir.join("secrets.json").exists());
    for entry in fs::read_dir(&accepted.dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|ext| ext == "json") {
            assert!(
                !fs::read_to_string(&path).unwrap().contains(&secrets.salt),
                "{}",
                path.display()
            );
        }
    }
    let stored = Capture::read(&accepted.dir).unwrap();
    assert!(matches!(&stored.witness, Witness::Proof(stored) if *stored == proof));

    let body = published(&router, &accepted.id).await;
    assert_eq!(body["phase"], "accepted", "{body}");
    assert_eq!(body["device_kind"], "software");
    assert_eq!(body["capture"]["location"]["simulated"], true);
    assert_eq!(body["claim"]["cell"], DEMO_PLACE.cell);
    let reused = body["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["event"] == "finished" && event["stage"] == "location-proof")
        .unwrap();
    assert!(
        reused["detail"]
            .as_str()
            .unwrap()
            .contains("made on the phone"),
        "{reused}"
    );

    let signed = manifest::read(&accepted.dir.join("signed.png")).unwrap();
    assert_eq!(
        signed.location_proof, proof.proof,
        "publishing proved again"
    );
    assert_eq!(signed.version, manifest::VERSION);
    let verdict = workspace.verifier().unwrap().verify(
        &accepted.dir.join("signed.png"),
        Some(DEMO_PLACE.cell),
        &mut |_| {},
    );
    assert!(verdict.rejected.is_none(), "{verdict:?}");
}

/// Runs `location-proof prove` as a separate process, the way the app's
/// library would produce the proof without the laptop.
fn prove_with_cli(
    binary: &Path,
    params: &Path,
    secrets: &provenance::capture::Secrets,
    cell: &str,
) -> LocationProof {
    let mut command = Command::new(binary);
    command
        .args(["prove", "--params"])
        .arg(params)
        .args(["--cell", cell]);
    serde_json::from_value(run_cli(command, secrets)).unwrap()
}

/// The hints the app computes with the same Go code and sends.
fn hints_with_cli(binary: &Path, coordinate: provenance::capture::Coordinate) -> [u32; 8] {
    let mut command = Command::new(binary);
    command.arg("hints");
    serde_json::from_value(run_cli(command, &coordinate)["hints"].take()).unwrap()
}

/// Runs a location-proof command with `input` as its JSON stdin.
fn run_cli(mut command: Command, input: &impl serde::Serialize) -> Value {
    use std::io::Write;
    let mut child = command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(input).unwrap())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs `provenance setup`; run with --ignored"]
async fn refuses_resolutions_outside_four_to_nine() {
    let home = scratch_home(&real_home());
    let workspace = Workspace::new(home.path());
    let tool = workspace.location_tool();
    let captures = tempfile::tempdir().unwrap();
    let router = Arc::new(Server::new(
        Workspace::new(home.path()),
        captures.path().to_path_buf(),
    ))
    .router(None);

    for resolution in [3, 10] {
        let (status, body) = post(
            &router,
            "/api/cell",
            &json!({"latitude": DEMO_PLACE.coordinate.latitude, "longitude": DEMO_PLACE.coordinate.longitude, "resolution": resolution}),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            error_of(&body).contains("outside the allowed range 4 to 9"),
            "{body}"
        );
    }

    let capture = Capture::simulate(
        &tool,
        &workspace.device_key().unwrap(),
        image::load_photo(&workspace.sample_photo()).unwrap(),
        DEMO_PLACE.coordinate,
        DEMO_PLACE.cell,
        crop_proof::Rect {
            x: 0,
            y: 0,
            width: 256,
            height: 128,
        },
    )
    .unwrap();
    let fine = fine_cell(&location_proof_cli(home.path()));
    let mut v1 = serde_json::to_value(upload_of(&capture)).unwrap();
    v1["cell"] = fine.clone().into();
    v1["resolution"] = 10.into();
    let (status, body) = post(&router, "/api/captures", &v1).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        error_of(&body).contains("outside the allowed range"),
        "{body}"
    );

    let secrets = capture.secrets().unwrap().clone();
    let proof = tool.prove(&secrets, &fine).unwrap();
    let v2 = ProofUpload {
        original_png: BASE64.encode(image::encode_png(&capture.original).unwrap()),
        receipt: capture.receipt.clone(),
        cell: fine.clone(),
        resolution: 10,
        crop: capture.summary.crop,
        turn: capture.summary.turn,
        accuracy_meters: None,
        location_proof: PhoneProof::Proof(proof.proof.clone()),
        location: None,
    };
    let (status, body) = post(
        &router,
        "/api/v2/captures",
        &serde_json::to_value(&v2).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        error_of(&body).contains("outside the allowed range"),
        "{body}"
    );

    // A file with a genuine proof for a resolution-10 cell, published past
    // the API: the reader refuses it at the location check.
    let mut summary = capture.summary.clone();
    summary.cell = fine.clone();
    summary.resolution = 10;
    let fine_capture = Capture {
        original: capture.original.clone(),
        witness: capture.witness.clone(),
        receipt: capture.receipt.clone(),
        summary,
    };
    let out = tempfile::tempdir().unwrap();
    let signed = publish::run(&workspace, &fine_capture, out.path(), &mut |_| {}).unwrap();
    let verdict = workspace
        .verifier()
        .unwrap()
        .verify(&signed, None, &mut |_| {});
    let rejected = verdict.rejected.expect("a resolution-10 cell was accepted");
    assert_eq!(rejected.check, Check::Location);
    assert!(
        rejected
            .detail
            .contains("resolution 10 is outside the allowed range 4 to 9"),
        "{}",
        rejected.detail
    );

    // The same file under assertion version 3 stops at the manifest.
    let mut old = manifest::read(&signed).unwrap();
    old.version = 3;
    let resigned = out.path().join("version-3.png");
    workspace
        .editor()
        .unwrap()
        .sign(&out.path().join("crop.png"), &old, &resigned)
        .unwrap();
    let verdict = workspace
        .verifier()
        .unwrap()
        .verify(&resigned, None, &mut |_| {});
    let rejected = verdict
        .rejected
        .expect("a version-3 assertion was accepted");
    assert_eq!(rejected.check, Check::Manifest);
    assert!(rejected.detail.contains("version 3"), "{}", rejected.detail);

    // Under the label used before, the manifest check still passes and says so.
    let legacy = out.path().join("legacy-label.png");
    workspace
        .editor()
        .unwrap()
        .sign_as(
            manifest::LEGACY_LABEL,
            &out.path().join("crop.png"),
            &manifest::read(&signed).unwrap(),
            &legacy,
        )
        .unwrap();
    let verdict = workspace
        .verifier()
        .unwrap()
        .verify(&legacy, None, &mut |_| {});
    let manifest_step = &verdict.passed[0];
    assert_eq!(manifest_step.check, Check::Manifest);
    assert!(
        manifest_step.detail.contains(manifest::LEGACY_LABEL),
        "{}",
        manifest_step.detail
    );
    assert_eq!(verdict.rejected.unwrap().check, Check::Location);
    // Under any other label there is nothing to read.
    let unknown = out.path().join("unknown-label.png");
    workspace
        .editor()
        .unwrap()
        .sign_as(
            "org.example.other",
            &out.path().join("crop.png"),
            &manifest::read(&signed).unwrap(),
            &unknown,
        )
        .unwrap();
    let verdict = workspace
        .verifier()
        .unwrap()
        .verify(&unknown, None, &mut |_| {});
    let rejected = verdict.rejected.unwrap();
    assert_eq!(rejected.check, Check::Manifest);
    assert!(rejected.detail.contains("found 0"), "{}", rejected.detail);
}

/// The resolution-10 cell of the demo coordinate, which the API refuses to
/// look up, straight from the location tool.
fn fine_cell(binary: &Path) -> String {
    let mut command = Command::new(binary);
    command.args(["cell", "--resolution", "10"]);
    run_cli(command, &DEMO_PLACE.coordinate)["cell"]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
#[ignore = "needs `provenance setup`; run with --ignored"]
fn serves_the_app_over_tls_with_the_pinned_key() {
    let home = scratch_home(&real_home());
    let workspace = Workspace::new(home.path());
    workspace.tls().create().unwrap();
    let tls = workspace.tls().load().unwrap();
    let free_port = || {
        std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    };
    let (port, tls_port) = (free_port(), free_port());
    let captures = tempfile::tempdir().unwrap();
    let router = Arc::new(
        Server::new(Workspace::new(home.path()), captures.path().to_path_buf()).lan(Lan {
            urls: vec![format!("https://localhost:{tls_port}")],
            pin: tls.pin.clone(),
        }),
    )
    .router(None);
    let config = tls.config.clone();
    std::thread::spawn(move || serve::run(router, port, Some((config, tls_port))));

    // curl checks the pin itself: sha256// and the base64 SPKI digest.
    let pin = format!("sha256//{}", BASE64.encode(hex::decode(&tls.pin).unwrap()));
    let curl = |pin: &str, path: &str| {
        let mut output = None;
        for _ in 0..50 {
            let result = Command::new("curl")
                .args(["-sS", "-k", "--pinnedpubkey", pin])
                .arg(format!("https://localhost:{tls_port}{path}"))
                .output()
                .unwrap();
            let refused = String::from_utf8_lossy(&result.stderr).contains("onnect");
            output = Some(result);
            if !refused {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        output.unwrap()
    };
    let output = curl(&pin, "/api/devices/00");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        r#"{"trusted":false}"#
    );
    // The pairing routes answer on the laptop over TLS too, and the
    // certificate names localhost.
    let output = curl(&pin, "/api/v2/pairing");
    let payload: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(payload["pin"], tls.pin.as_str(), "{payload}");
    let output = curl(&pin, "/api/v2/session");
    let session: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(session["qr"]["pin"], tls.pin.as_str(), "{session}");
    let wrong = format!("sha256//{}", BASE64.encode([0u8; 32]));
    let output = curl(&wrong, "/api/devices/00");
    assert!(!output.status.success(), "curl accepted another key");
}
