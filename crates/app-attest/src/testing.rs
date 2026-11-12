//! Attestation objects in Apple's format, under a test root instead of Apple's.

use ciborium::value::Value;
use rcgen::{
    date_time_ymd, BasicConstraints, CertificateParams, CertifiedIssuer, CustomExtension, DnType,
    IsCa, KeyPair, PKCS_ECDSA_P256_SHA256, PKCS_ECDSA_P384_SHA384,
};

use crate::{sha256, AAGUID_DEVELOPMENT, FLAG_ATTESTED, FLAG_EXTENSIONS};

/// A test root and intermediate, P-384 like Apple's.
pub struct Forge {
    root: Vec<u8>,
    intermediate: CertifiedIssuer<'static, KeyPair>,
}

/// What to put in a forged attestation. [`Options::new`] makes a valid one.
#[derive(Clone)]
pub struct Options {
    pub app_id: String,
    pub client_data_hash: Vec<u8>,
    pub counter: u32,
    pub aaguid: [u8; 16],
    /// Leaves out the extensions map when `None`.
    pub validation_category: Option<u32>,
    /// Replaces the nonce in the credential certificate.
    pub nonce: Option<Vec<u8>>,
    /// Puts a different credential id in the authenticator data.
    pub credential_id: Option<Vec<u8>>,
    /// Makes the credential certificate expire in 2000.
    pub expired: bool,
}

impl Options {
    pub fn new(app_id: &str, client_data_hash: &[u8]) -> Self {
        Self {
            app_id: app_id.to_string(),
            client_data_hash: client_data_hash.to_vec(),
            counter: 0,
            aaguid: *AAGUID_DEVELOPMENT,
            // A build signed by a development identity.
            validation_category: Some(3),
            nonce: None,
            credential_id: None,
            expired: false,
        }
    }
}

pub struct Forged {
    /// The CBOR attestation object.
    pub object: Vec<u8>,
    /// SHA-256 of the attested key's uncompressed point.
    pub key_id: Vec<u8>,
}

impl Default for Forge {
    fn default() -> Self {
        Self::new()
    }
}

impl Forge {
    pub fn new() -> Self {
        let mut root = CertificateParams::new(Vec::<String>::new()).unwrap();
        root.distinguished_name
            .push(DnType::CommonName, "Test App Attestation Root CA");
        root.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        let root = CertifiedIssuer::self_signed(
            root,
            KeyPair::generate_for(&PKCS_ECDSA_P384_SHA384).unwrap(),
        )
        .unwrap();

        let mut intermediate = CertificateParams::new(Vec::<String>::new()).unwrap();
        intermediate
            .distinguished_name
            .push(DnType::CommonName, "Test App Attestation CA 1");
        intermediate.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
        let intermediate = CertifiedIssuer::signed_by(
            intermediate,
            KeyPair::generate_for(&PKCS_ECDSA_P384_SHA384).unwrap(),
            &root,
        )
        .unwrap();

        Self {
            root: root.der().to_vec(),
            intermediate,
        }
    }

    /// The root, DER, to pass to [`crate::verify_with_root`].
    pub fn root(&self) -> &[u8] {
        &self.root
    }

    pub fn attest(&self, options: &Options) -> Forged {
        let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
        let point = key.public_key_raw().to_vec();
        let key_id = sha256(&[&point]);
        let credential_id = options
            .credential_id
            .clone()
            .unwrap_or_else(|| key_id.clone());

        let mut auth_data = sha256(&[options.app_id.as_bytes()]);
        let mut flags = FLAG_ATTESTED;
        if options.validation_category.is_some() {
            flags |= FLAG_EXTENSIONS;
        }
        auth_data.push(flags);
        auth_data.extend_from_slice(&options.counter.to_be_bytes());
        auth_data.extend_from_slice(&options.aaguid);
        auth_data.extend_from_slice(&(credential_id.len() as u16).to_be_bytes());
        auth_data.extend_from_slice(&credential_id);
        let cose = Value::Map(vec![
            (1.into(), 2.into()),
            (3.into(), (-7).into()),
            ((-1).into(), 1.into()),
            ((-2).into(), Value::Bytes(point[1..33].to_vec())),
            ((-3).into(), Value::Bytes(point[33..].to_vec())),
        ]);
        ciborium::into_writer(&cose, &mut auth_data).unwrap();
        if let Some(category) = options.validation_category {
            let extensions = Value::Map(vec![
                ("apple_bundle_version_01".into(), "1".into()),
                (
                    "apple_validation_category_01".into(),
                    Value::Bytes(category.to_le_bytes().to_vec()),
                ),
            ]);
            ciborium::into_writer(&extensions, &mut auth_data).unwrap();
        }

        let nonce = options
            .nonce
            .clone()
            .unwrap_or_else(|| sha256(&[&auth_data, &options.client_data_hash]));
        assert_eq!(nonce.len(), 32);
        // SEQUENCE { [1] { OCTET STRING nonce } }, as in Apple's sample.
        let mut extension = vec![0x30, 0x24, 0xa1, 0x22, 0x04, 0x20];
        extension.extend_from_slice(&nonce);

        let mut leaf = CertificateParams::new(Vec::<String>::new()).unwrap();
        leaf.distinguished_name
            .push(DnType::CommonName, hex(&key_id));
        leaf.custom_extensions
            .push(CustomExtension::from_oid_content(
                &[1, 2, 840, 113635, 100, 8, 2],
                extension,
            ));
        if options.expired {
            leaf.not_before = date_time_ymd(1999, 1, 1);
            leaf.not_after = date_time_ymd(2000, 1, 1);
        }
        let leaf = leaf.signed_by(&key, &self.intermediate).unwrap();

        let object = Value::Map(vec![
            ("fmt".into(), "apple-appattest".into()),
            (
                "attStmt".into(),
                Value::Map(vec![
                    (
                        "x5c".into(),
                        Value::Array(vec![
                            Value::Bytes(leaf.der().to_vec()),
                            Value::Bytes(self.intermediate.der().to_vec()),
                        ]),
                    ),
                    ("receipt".into(), Value::Bytes(b"test receipt".to_vec())),
                ]),
            ),
            ("authData".into(), Value::Bytes(auth_data)),
        ]);
        let mut bytes = Vec::new();
        ciborium::into_writer(&object, &mut bytes).unwrap();
        Forged {
            object: bytes,
            key_id,
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
