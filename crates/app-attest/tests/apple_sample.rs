//! Apple's own sample attestation object, from the Attestation Object
//! Validation Guide. Its credential certificate is valid from 2026-04-20
//! 18:13:12 to 2026-04-23 18:13:12 UTC, so it is checked at a fixed time.

use app_attest::{
    apple_root,
    receipt::{apple_root_g3, verify_receipt, verify_receipt_with_root, ReceiptKind},
    verify, Environment, Error, Expected,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use sha2::{Digest, Sha256};

/// 2026-04-21T12:00:00Z.
const INSIDE: i64 = 1_776_772_800;
/// 2026-11-05T00:00:00Z.
const AFTER: i64 = 1_793_836_800;

#[derive(serde::Deserialize)]
struct Sample {
    team_id: String,
    bundle_id: String,
    client_data_hash: String,
    key_id: String,
    attestation: String,
}

fn sample() -> (Sample, Vec<u8>, Vec<u8>) {
    let sample: Sample = serde_json::from_str(include_str!("fixtures/apple-sample.json")).unwrap();
    let object = BASE64.decode(&sample.attestation).unwrap();
    let key_id = BASE64.decode(&sample.key_id).unwrap();
    (sample, object, key_id)
}

#[test]
fn bundled_root_is_apples() {
    // SHA-256 of the DER that apple.com serves, checked with OpenSSL on 2026-11-05.
    assert_eq!(
        hex(&Sha256::digest(apple_root())),
        "1cb9823ba28ba6ad2d33a006941de2ae4f513ef1d4e831b9f7e0fa7b6242c932"
    );
}

#[test]
fn verifies_inside_its_validity() {
    let (sample, object, key_id) = sample();
    let app_id = format!("{}.{}", sample.team_id, sample.bundle_id);
    // In the guide, the composite that is hashed into the nonce ends with the
    // challenge's own bytes, so the sample app passed them as clientDataHash.
    let attestation = verify(
        &object,
        &Expected {
            app_id: &app_id,
            key_id: &key_id,
            client_data_hash: sample.client_data_hash.as_bytes(),
        },
        INSIDE,
    )
    .unwrap();
    assert_eq!(attestation.environment, Environment::Production);
    assert_eq!(attestation.validation_category, Some(1));
    // The guide's text says "1.0"; the sample object carries "1".
    assert_eq!(attestation.bundle_version.as_deref(), Some("1"));
    assert_eq!(hex(&Sha256::digest(&attestation.public_key)), hex(&key_id));
    assert!(!attestation.receipt.is_empty());
}

#[test]
fn refuses_after_the_certificate_expired() {
    let (sample, object, key_id) = sample();
    let app_id = format!("{}.{}", sample.team_id, sample.bundle_id);
    let expected = Expected {
        app_id: &app_id,
        key_id: &key_id,
        client_data_hash: sample.client_data_hash.as_bytes(),
    };
    assert!(matches!(
        verify(&object, &expected, AFTER),
        Err(Error::Chain(_))
    ));
}

#[test]
fn refuses_another_app_challenge_or_key() {
    let (sample, object, key_id) = sample();
    let app_id = format!("{}.{}", sample.team_id, sample.bundle_id);
    let challenge = sample.client_data_hash.as_bytes();

    let other_app = Expected {
        app_id: "1234567890.com.example.other",
        key_id: &key_id,
        client_data_hash: challenge,
    };
    assert_eq!(
        verify(&object, &other_app, INSIDE).unwrap_err(),
        Error::AppId
    );

    let other_challenge = Expected {
        app_id: &app_id,
        key_id: &key_id,
        client_data_hash: b"another_server_challenge",
    };
    assert_eq!(
        verify(&object, &other_challenge, INSIDE).unwrap_err(),
        Error::Nonce
    );

    let mut other_key = key_id.clone();
    other_key[0] ^= 1;
    let other_key = Expected {
        app_id: &app_id,
        key_id: &other_key,
        client_data_hash: challenge,
    };
    assert_eq!(
        verify(&object, &other_key, INSIDE).unwrap_err(),
        Error::KeyId
    );
}

#[test]
fn refuses_a_truncated_or_padded_object() {
    let (sample, object, key_id) = sample();
    let app_id = format!("{}.{}", sample.team_id, sample.bundle_id);
    let expected = Expected {
        app_id: &app_id,
        key_id: &key_id,
        client_data_hash: sample.client_data_hash.as_bytes(),
    };
    assert!(matches!(
        verify(&object[..object.len() - 1], &expected, INSIDE),
        Err(Error::Format(_))
    ));
    let mut padded = object.clone();
    padded.push(0);
    assert!(matches!(
        verify(&padded, &expected, INSIDE),
        Err(Error::Format(_))
    ));
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// 2027-03-01T00:00:00Z, after the receipt signer expires on 2027-02-18.
const SIGNER_EXPIRED: i64 = 1_803_859_200;

fn sample_receipt() -> (Vec<u8>, Vec<u8>, String) {
    let (sample, object, key_id) = sample();
    let app_id = format!("{}.{}", sample.team_id, sample.bundle_id);
    let attestation = verify(
        &object,
        &Expected {
            app_id: &app_id,
            key_id: &key_id,
            client_data_hash: sample.client_data_hash.as_bytes(),
        },
        INSIDE,
    )
    .unwrap();
    (attestation.receipt, attestation.public_key, app_id)
}

#[test]
fn bundled_receipt_root_is_apple_root_g3() {
    // SHA-256 of https://www.apple.com/certificateauthority/AppleRootCA-G3.cer,
    // checked with OpenSSL on 2026-11-05.
    assert_eq!(
        hex(&Sha256::digest(apple_root_g3())),
        "63343abfb89a6a03ebb57e9b3f5fa7be7c4f5c756f3017b3a8c488c3653e9179"
    );
}

#[test]
fn verifies_the_sample_receipt() {
    let (der, public_key, app_id) = sample_receipt();
    let receipt = verify_receipt(&der, INSIDE).unwrap();
    assert_eq!(receipt.kind, ReceiptKind::Attest);
    assert_eq!(receipt.app_id, app_id);
    assert_eq!(receipt.environment.as_deref(), Some("production"));
    // 2026-04-21T18:13:12Z.
    assert_eq!(receipt.created, 1_776_795_192);
    assert_eq!(receipt.risk_metric, None);
    assert!(receipt.attests(&public_key));
}

#[test]
fn refuses_the_sample_receipt_after_its_signer_expires_or_under_another_root() {
    let (der, _, _) = sample_receipt();
    assert!(matches!(
        verify_receipt(&der, SIGNER_EXPIRED),
        Err(Error::Chain(_))
    ));
    assert!(matches!(
        verify_receipt_with_root(&der, INSIDE, &apple_root()),
        Err(Error::Chain(_))
    ));
    let mut flipped = der.clone();
    let at = flipped.len() / 3;
    flipped[at] ^= 1;
    assert!(verify_receipt(&flipped, INSIDE).is_err());
}
