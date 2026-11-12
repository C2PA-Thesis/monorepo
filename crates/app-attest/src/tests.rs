//! Each check against attestations forged under a test root.

use ciborium::value::Value;

use super::*;
use crate::testing::{Forge, Options};

type CborMap = Vec<(Value, Value)>;

const APP_ID: &str = "ABCDE12345.io.github.c2pa-thesis.capture";
const NOW: i64 = 1_793_836_800;

fn check(forge: &Forge, options: &Options) -> Result<Attestation, Error> {
    let forged = forge.attest(options);
    verify_with_root(
        &forged.object,
        &Expected {
            app_id: APP_ID,
            key_id: &forged.key_id,
            client_data_hash: &sha256(&[b"challenge"]),
        },
        NOW,
        forge.root(),
    )
}

fn valid() -> Options {
    Options::new(APP_ID, &sha256(&[b"challenge"]))
}

#[test]
fn accepts_a_valid_one() {
    let attestation = check(&Forge::new(), &valid()).unwrap();
    assert_eq!(attestation.environment, Environment::Development);
    assert_eq!(attestation.validation_category, Some(3));
    assert_eq!(attestation.bundle_version.as_deref(), Some("1"));
    assert!(!attestation.receipt.is_empty());
}

#[test]
fn accepts_production_and_no_extensions() {
    let forge = Forge::new();
    let options = Options {
        aaguid: *AAGUID_PRODUCTION,
        validation_category: None,
        ..valid()
    };
    let attestation = check(&forge, &options).unwrap();
    assert_eq!(attestation.environment, Environment::Production);
    assert_eq!(attestation.validation_category, None);
}

#[test]
fn refuses_another_root() {
    let forge = Forge::new();
    let forged = forge.attest(&valid());
    let expected = Expected {
        app_id: APP_ID,
        key_id: &forged.key_id,
        client_data_hash: &sha256(&[b"challenge"]),
    };
    assert!(matches!(
        verify_with_root(&forged.object, &expected, NOW, Forge::new().root()),
        Err(Error::Chain(_))
    ));
    assert!(matches!(
        verify(&forged.object, &expected, NOW),
        Err(Error::Chain(_))
    ));
}

#[test]
fn refuses_an_expired_leaf() {
    let options = Options {
        expired: true,
        ..valid()
    };
    assert!(matches!(
        check(&Forge::new(), &options),
        Err(Error::Chain(_))
    ));
}

#[test]
fn refuses_a_wrong_nonce() {
    let options = Options {
        nonce: Some(vec![7; 32]),
        ..valid()
    };
    assert_eq!(check(&Forge::new(), &options).unwrap_err(), Error::Nonce);
}

#[test]
fn refuses_another_client_data_hash() {
    let options = Options::new(APP_ID, &sha256(&[b"another challenge"]));
    assert_eq!(check(&Forge::new(), &options).unwrap_err(), Error::Nonce);
}

#[test]
fn refuses_another_app_id() {
    let options = Options::new("ABCDE12345.com.example.other", &sha256(&[b"challenge"]));
    assert_eq!(check(&Forge::new(), &options).unwrap_err(), Error::AppId);
}

#[test]
fn refuses_a_used_counter() {
    let options = Options {
        counter: 1,
        ..valid()
    };
    assert_eq!(
        check(&Forge::new(), &options).unwrap_err(),
        Error::Counter(1)
    );
}

#[test]
fn refuses_an_unknown_aaguid() {
    let options = Options {
        aaguid: *b"appattestsandbox",
        ..valid()
    };
    assert_eq!(
        check(&Forge::new(), &options).unwrap_err(),
        Error::Environment
    );
}

#[test]
fn refuses_another_credential_id() {
    let options = Options {
        credential_id: Some(vec![9; 32]),
        ..valid()
    };
    assert_eq!(
        check(&Forge::new(), &options).unwrap_err(),
        Error::CredentialId
    );
}

#[test]
fn refuses_categories_apple_calls_invalid() {
    let forge = Forge::new();
    for category in [0, 7, 8, 9, 11] {
        let options = Options {
            validation_category: Some(category),
            ..valid()
        };
        assert_eq!(
            check(&forge, &options).unwrap_err(),
            Error::Category(category)
        );
    }
}

#[test]
fn refuses_altered_authenticator_data() {
    let forge = Forge::new();
    let forged = forge.attest(&valid());
    let mut value: Value = ciborium::from_reader(&forged.object[..]).unwrap();
    for (key, field) in value.as_map_mut().unwrap() {
        if key.as_text() == Some("authData") {
            if let Value::Bytes(bytes) = field {
                // A byte of the counter.
                bytes[36] ^= 1;
            }
        }
    }
    let mut object = Vec::new();
    ciborium::into_writer(&value, &mut object).unwrap();
    let expected = Expected {
        app_id: APP_ID,
        key_id: &forged.key_id,
        client_data_hash: &sha256(&[b"challenge"]),
    };
    assert_eq!(
        verify_with_root(&object, &expected, NOW, forge.root()).unwrap_err(),
        Error::Nonce
    );
}

#[test]
fn refuses_another_fmt_and_a_short_chain() {
    let forge = Forge::new();
    let forged = forge.attest(&valid());
    let expected = Expected {
        app_id: APP_ID,
        key_id: &forged.key_id,
        client_data_hash: &sha256(&[b"challenge"]),
    };
    let edit = |change: &dyn Fn(&mut CborMap)| {
        let mut value: Value = ciborium::from_reader(&forged.object[..]).unwrap();
        change(value.as_map_mut().unwrap());
        let mut object = Vec::new();
        ciborium::into_writer(&value, &mut object).unwrap();
        verify_with_root(&object, &expected, NOW, forge.root())
    };
    let packed = edit(&|map| map[0].1 = "packed".into());
    assert!(matches!(packed, Err(Error::Format(_))));
    let short = edit(&|map| {
        let statement = map[1].1.as_map_mut().unwrap();
        statement[0].1.as_array_mut().unwrap().truncate(1);
    });
    assert!(matches!(short, Err(Error::Chain(_))));
}

#[test]
fn reads_long_form_der_lengths() {
    let mut long = vec![0x04, 0x81, 0x80];
    long.extend_from_slice(&[1; 128]);
    assert_eq!(der_only(&long, 0x04).unwrap().len(), 128);
    assert!(der_only(&long[..long.len() - 1], 0x04).is_none());
    assert!(der_only(&long, 0x30).is_none());
}
