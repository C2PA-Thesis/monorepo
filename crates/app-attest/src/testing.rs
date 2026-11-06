//! Attestation objects and receipts in Apple's format, under a test root
//! instead of Apple's.

use ciborium::value::Value;
use rcgen::{
    date_time_ymd, BasicConstraints, CertificateParams, CertifiedIssuer, CustomExtension, DnType,
    IsCa, KeyPair, SigningKey as _, PKCS_ECDSA_P256_SHA256, PKCS_ECDSA_P384_SHA384,
};
use x509_parser::{certificate::X509Certificate, prelude::FromDer};

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
    /// The credential certificate, DER.
    pub credential_certificate: Vec<u8>,
}

/// What to put in a forged receipt. [`ReceiptOptions::new`] makes a valid
/// `RECEIPT` with a risk metric of 1.
#[derive(Clone)]
pub struct ReceiptOptions {
    pub app_id: String,
    /// Field 3, DER.
    pub attested_certificate: Vec<u8>,
    /// Field 6.
    pub kind: String,
    /// Field 7.
    pub environment: Option<String>,
    /// Field 12, Unix seconds.
    pub created: i64,
    /// Field 17.
    pub risk_metric: Option<u32>,
    /// The signing certificate's common name.
    pub signer_name: String,
    /// Changes one payload byte after signing.
    pub tampered: bool,
}

impl ReceiptOptions {
    pub fn new(app_id: &str, attested_certificate: &[u8], created: i64) -> Self {
        Self {
            app_id: app_id.to_string(),
            attested_certificate: attested_certificate.to_vec(),
            kind: "RECEIPT".into(),
            environment: Some("sandbox".into()),
            created,
            risk_metric: Some(1),
            signer_name: "Application Attestation Fraud Receipt Signing".into(),
            tampered: false,
        }
    }
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
        let mut receipt = ReceiptOptions::new(&options.app_id, leaf.der(), now());
        receipt.kind = "ATTEST".into();
        receipt.risk_metric = None;
        let receipt = self.receipt(&receipt);

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
                    ("receipt".into(), Value::Bytes(receipt)),
                ]),
            ),
            ("authData".into(), Value::Bytes(auth_data)),
        ]);
        let mut bytes = Vec::new();
        ciborium::into_writer(&object, &mut bytes).unwrap();
        Forged {
            object: bytes,
            key_id,
            credential_certificate: leaf.der().to_vec(),
        }
    }

    /// A PKCS #7 signed receipt, DER, signed by a leaf under the test root.
    pub fn receipt(&self, options: &ReceiptOptions) -> Vec<u8> {
        let mut fields = vec![
            field(2, options.app_id.as_bytes()),
            field(3, &options.attested_certificate),
            field(4, &[0u8; 32]),
            field(5, b"test token"),
            field(6, options.kind.as_bytes()),
            field(12, format_time(options.created).as_bytes()),
            field(19, format_time(options.created + 86_400).as_bytes()),
            field(21, format_time(options.created + 90 * 86_400).as_bytes()),
        ];
        if let Some(environment) = &options.environment {
            fields.push(field(7, environment.as_bytes()));
        }
        if let Some(metric) = options.risk_metric {
            fields.push(field(17, metric.to_string().as_bytes()));
        }
        let mut payload = tlv(0x31, &fields.concat());

        let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
        let mut signer = CertificateParams::new(Vec::<String>::new()).unwrap();
        signer
            .distinguished_name
            .push(DnType::CommonName, options.signer_name.clone());
        let signer = signer.signed_by(&key, &self.intermediate).unwrap();
        let signature = key.sign(&payload).unwrap();
        if options.tampered {
            let last = payload.len() - 1;
            payload[last] ^= 1;
        }
        let (_, parsed) = X509Certificate::from_der(signer.der()).unwrap();

        const SHA256: &[u8] = &[0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01];
        const ECDSA_SHA256: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x02];
        const DATA: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x07, 0x01];
        const SIGNED_DATA: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x07, 0x02];
        let algorithm = |oid| tlv(0x30, &tlv(0x06, oid));
        let signer_info = tlv(
            0x30,
            &[
                tlv(0x02, &[1]),
                tlv(
                    0x30,
                    &[
                        parsed.issuer().as_raw().to_vec(),
                        tlv(0x02, parsed.raw_serial()),
                    ]
                    .concat(),
                ),
                algorithm(SHA256),
                algorithm(ECDSA_SHA256),
                tlv(0x04, &signature),
            ]
            .concat(),
        );
        let signed_data = tlv(
            0x30,
            &[
                tlv(0x02, &[1]),
                tlv(0x31, &algorithm(SHA256)),
                tlv(
                    0x30,
                    &[tlv(0x06, DATA), tlv(0xa0, &tlv(0x04, &payload))].concat(),
                ),
                tlv(
                    0xa0,
                    &[signer.der().to_vec(), self.intermediate.der().to_vec()].concat(),
                ),
                tlv(0x31, &signer_info),
            ]
            .concat(),
        );
        tlv(
            0x30,
            &[tlv(0x06, SIGNED_DATA), tlv(0xa0, &signed_data)].concat(),
        )
    }
}

fn field(kind: u8, value: &[u8]) -> Vec<u8> {
    tlv(
        0x30,
        &[tlv(0x02, &[kind]), tlv(0x02, &[1]), tlv(0x04, value)].concat(),
    )
}

fn tlv(tag: u8, content: &[u8]) -> Vec<u8> {
    let mut out = vec![tag];
    let len = content.len();
    if len < 0x80 {
        out.push(len as u8);
    } else {
        let bytes: Vec<u8> = len
            .to_be_bytes()
            .into_iter()
            .skip_while(|b| *b == 0)
            .collect();
        out.push(0x80 | bytes.len() as u8);
        out.extend(bytes);
    }
    out.extend_from_slice(content);
    out
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

/// Unix seconds as `YYYY-MM-DDTHH:MM:SS.000Z`.
fn format_time(seconds: i64) -> String {
    let days = seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400);
    // Civil date from days since 1970-01-01, proleptic Gregorian.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.000Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
