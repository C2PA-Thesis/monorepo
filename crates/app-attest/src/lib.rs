//! Verifies an Apple App Attest attestation object, following the steps of
//! Apple's "Validating apps that connect to your server", section "Verify the
//! attestation". See README.md for what each step checks and what it does not.

use std::fmt;

use ciborium::value::Value;
use sha2::{Digest, Sha256};
use x509_parser::{certificate::X509Certificate, prelude::FromDer, time::ASN1Time};

pub mod jwt;
pub mod receipt;
#[cfg(any(test, feature = "testing"))]
pub mod testing;

/// Apple App Attestation Root CA, from
/// https://www.apple.com/certificateauthority/Apple_App_Attestation_Root_CA.pem
const APPLE_ROOT_PEM: &[u8] = include_bytes!("apple-app-attestation-root-ca.pem");

/// The credential certificate extension that holds the nonce.
const NONCE_OID: &str = "1.2.840.113635.100.8.2";

const AAGUID_PRODUCTION: &[u8; 16] = b"appattest\0\0\0\0\0\0\0";
const AAGUID_DEVELOPMENT: &[u8; 16] = b"appattestdevelop";

/// Authenticator data flag: attested credential data follows the counter.
const FLAG_ATTESTED: u8 = 0x40;
/// Authenticator data flag: an extensions map follows the credential.
const FLAG_EXTENSIONS: u8 = 0x80;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Environment {
    Development,
    Production,
}

impl Environment {
    pub fn as_str(self) -> &'static str {
        match self {
            Environment::Development => "development",
            Environment::Production => "production",
        }
    }
}

/// What the server compares the attestation against.
pub struct Expected<'a> {
    /// Team ID, a period, and the bundle id.
    pub app_id: &'a str,
    /// The key identifier the app got from `generateKey`, decoded from base64.
    pub key_id: &'a [u8],
    /// The `clientDataHash` the app passed to `attestKey`.
    pub client_data_hash: &'a [u8],
}

/// A verified attestation.
#[derive(Clone, Debug)]
pub struct Attestation {
    /// The attested key, as a 65-byte uncompressed P-256 point.
    pub public_key: Vec<u8>,
    pub environment: Environment,
    /// `apple_validation_category_01`, when the extensions carry it.
    pub validation_category: Option<u32>,
    /// `apple_bundle_version_01`, when the extensions carry it. Recorded, not checked.
    pub bundle_version: Option<String>,
    /// For Apple's fraud metric. Not verified here.
    pub receipt: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Format(&'static str),
    Chain(&'static str),
    Nonce,
    KeyId,
    AppId,
    Counter(u32),
    Environment,
    CredentialId,
    CoseKey,
    Category(u32),
    Receipt(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Format(what) => write!(f, "the attestation object is malformed: {what}"),
            Error::Chain(what) => write!(f, "the certificate chain does not lead to the pinned Apple root: {what}"),
            Error::Nonce => write!(f, "the nonce in the credential certificate does not match the authenticator data and client data hash"),
            Error::KeyId => write!(f, "the credential certificate's key does not hash to the key id"),
            Error::AppId => write!(f, "the RP ID is not the hash of the expected App ID"),
            Error::Counter(counter) => write!(f, "the counter is {counter}, not 0"),
            Error::Environment => write!(f, "the aaguid is neither App Attest development nor production"),
            Error::CredentialId => write!(f, "the credential id is not the key id"),
            Error::CoseKey => write!(f, "the COSE key in the authenticator data is not the certificate's key"),
            Error::Category(category) => write!(f, "validation category {category} is not accepted"),
            Error::Receipt(what) => write!(f, "the receipt does not verify: {what}"),
        }
    }
}

impl std::error::Error for Error {}

/// Apple's root certificate, DER.
pub fn apple_root() -> Vec<u8> {
    let (_, pem) =
        x509_parser::pem::parse_x509_pem(APPLE_ROOT_PEM).expect("the bundled root is PEM");
    pem.contents
}

/// Verifies `object` against Apple's root at `now` (Unix seconds).
pub fn verify(object: &[u8], expected: &Expected, now: i64) -> Result<Attestation, Error> {
    verify_with_root(object, expected, now, &apple_root())
}

/// The same checks against another root. Only tests pass anything but [`apple_root`].
pub fn verify_with_root(
    object: &[u8],
    expected: &Expected,
    now: i64,
    root: &[u8],
) -> Result<Attestation, Error> {
    let decoded = Decoded::parse(object)?;

    // Step 1: the chain.
    let leaf = check_chain(&decoded.x5c, root, now)?;

    // Steps 2 to 4: nonce = SHA256(authData || clientDataHash) is in credCert.
    let nonce = sha256(&[&decoded.auth_data, expected.client_data_hash]);
    if nonce_in(&leaf)? != nonce {
        return Err(Error::Nonce);
    }

    // Step 5: the key hashes to the key id.
    let public_key = leaf.public_key().subject_public_key.data.to_vec();
    if public_key.len() != 65 || public_key[0] != 4 {
        return Err(Error::Format(
            "the credential key is not an uncompressed P-256 point",
        ));
    }
    if sha256(&[&public_key]) != expected.key_id {
        return Err(Error::KeyId);
    }

    let auth = AuthData::parse(&decoded.auth_data)?;

    // Step 6: the RP ID.
    if auth.rp_id_hash != sha256(&[expected.app_id.as_bytes()]) {
        return Err(Error::AppId);
    }
    // Step 7: the counter.
    if auth.counter != 0 {
        return Err(Error::Counter(auth.counter));
    }
    // Step 8: the environment.
    let environment = match &auth.aaguid {
        a if a == AAGUID_PRODUCTION => Environment::Production,
        a if a == AAGUID_DEVELOPMENT => Environment::Development,
        _ => return Err(Error::Environment),
    };
    // Step 9: the credential id.
    if auth.credential_id != expected.key_id {
        return Err(Error::CredentialId);
    }
    // Not one of Apple's steps: the COSE key must be the certificate's key.
    if auth.cose_point()? != public_key {
        return Err(Error::CoseKey);
    }
    // Steps 10 and 11: the extensions.
    let (validation_category, bundle_version) = auth.extensions()?;
    if let Some(category) = validation_category {
        // 0 is "Invalid"; 7, 8 and 9 "aren't appropriate"; Apple lists none above 10.
        if !matches!(category, 1..=6 | 10) {
            return Err(Error::Category(category));
        }
    }

    Ok(Attestation {
        public_key,
        environment,
        validation_category,
        bundle_version,
        receipt: decoded.receipt,
    })
}

struct Decoded {
    x5c: Vec<Vec<u8>>,
    receipt: Vec<u8>,
    auth_data: Vec<u8>,
}

impl Decoded {
    fn parse(object: &[u8]) -> Result<Self, Error> {
        let mut rest = object;
        let value: Value =
            ciborium::from_reader(&mut rest).map_err(|_| Error::Format("not CBOR"))?;
        if !rest.is_empty() {
            return Err(Error::Format("trailing bytes after the CBOR object"));
        }
        let map = value.as_map().ok_or(Error::Format("not a CBOR map"))?;
        if text(get(map, "fmt")?) != Some("apple-appattest") {
            return Err(Error::Format("fmt is not apple-appattest"));
        }
        let statement = get(map, "attStmt")?
            .as_map()
            .ok_or(Error::Format("attStmt is not a map"))?;
        let x5c = get(statement, "x5c")?
            .as_array()
            .ok_or(Error::Format("x5c is not an array"))?
            .iter()
            .map(|cert| {
                cert.as_bytes()
                    .cloned()
                    .ok_or(Error::Format("x5c holds a non-byte entry"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let receipt = bytes(get(statement, "receipt")?)?;
        let auth_data = bytes(get(map, "authData")?)?;
        Ok(Self {
            x5c,
            receipt,
            auth_data,
        })
    }
}

fn get<'a>(map: &'a [(Value, Value)], key: &str) -> Result<&'a Value, Error> {
    map.iter()
        .find(|(k, _)| text(k) == Some(key))
        .map(|(_, v)| v)
        .ok_or(Error::Format("a required field is missing"))
}

fn text(value: &Value) -> Option<&str> {
    value.as_text()
}

fn bytes(value: &Value) -> Result<Vec<u8>, Error> {
    value
        .as_bytes()
        .cloned()
        .ok_or(Error::Format("a byte field is not bytes"))
}

/// Checks leaf, intermediates and the pinned root, and returns the leaf.
fn check_chain<'a>(
    x5c: &'a [Vec<u8>],
    root: &'a [u8],
    now: i64,
) -> Result<X509Certificate<'a>, Error> {
    if x5c.len() < 2 {
        return Err(Error::Chain(
            "the chain needs a leaf and an intermediate certificate",
        ));
    }
    let at = ASN1Time::from_timestamp(now).map_err(|_| Error::Chain("bad time"))?;
    let parse = |der: &'a [u8]| {
        X509Certificate::from_der(der)
            .map_err(|_| Error::Chain("a certificate does not parse"))
            .and_then(|(rest, cert)| {
                if rest.is_empty() {
                    Ok(cert)
                } else {
                    Err(Error::Chain("trailing bytes after a certificate"))
                }
            })
    };
    let mut chain = x5c
        .iter()
        .map(|der| parse(der))
        .collect::<Result<Vec<_>, _>>()?;
    let root = parse(root)?;
    for (index, cert) in chain.iter().enumerate() {
        if !cert.validity().is_valid_at(at) {
            return Err(Error::Chain("a certificate is expired or not yet valid"));
        }
        let issuer = chain.get(index + 1).unwrap_or(&root);
        if cert.issuer().as_raw() != issuer.subject().as_raw() {
            return Err(Error::Chain(
                "a certificate's issuer is not the next certificate",
            ));
        }
        let is_ca = issuer
            .basic_constraints()
            .ok()
            .flatten()
            .is_some_and(|ext| ext.value.ca);
        if !is_ca {
            return Err(Error::Chain("an issuer is not a CA"));
        }
        cert.verify_signature(Some(issuer.public_key()))
            .map_err(|_| Error::Chain("a signature does not verify"))?;
    }
    Ok(chain.swap_remove(0))
}

/// The octet string inside the nonce extension: SEQUENCE { [1] { OCTET STRING } }.
fn nonce_in(leaf: &X509Certificate) -> Result<Vec<u8>, Error> {
    let missing = Error::Format("the credential certificate has no nonce extension");
    let extension = leaf
        .extensions()
        .iter()
        .find(|ext| ext.oid.to_id_string() == NONCE_OID)
        .ok_or(missing)?;
    let bad = Error::Format("the nonce extension is not a sequence holding one octet string");
    let sequence = der_only(extension.value, 0x30).ok_or(bad.clone())?;
    let tagged = der_only(sequence, 0xa1).ok_or(bad.clone())?;
    Ok(der_only(tagged, 0x04).ok_or(bad)?.to_vec())
}

/// The content of `input` when it is exactly one DER element with `tag`.
fn der_only(input: &[u8], tag: u8) -> Option<&[u8]> {
    let (&first, rest) = input.split_first()?;
    if first != tag {
        return None;
    }
    let (&len, rest) = rest.split_first()?;
    let (len, rest) = if len < 0x80 {
        (len as usize, rest)
    } else {
        let count = (len & 0x7f) as usize;
        if count == 0 || count > 2 || rest.len() < count {
            return None;
        }
        let len = rest[..count]
            .iter()
            .fold(0usize, |acc, b| acc << 8 | *b as usize);
        (len, &rest[count..])
    };
    (rest.len() == len).then_some(rest)
}

struct AuthData {
    rp_id_hash: Vec<u8>,
    counter: u32,
    aaguid: [u8; 16],
    credential_id: Vec<u8>,
    cose_key: Value,
    extensions: Option<Value>,
}

impl AuthData {
    fn parse(data: &[u8]) -> Result<Self, Error> {
        let short = Error::Format("the authenticator data is too short");
        if data.len() < 37 + 18 {
            return Err(short);
        }
        let flags = data[32];
        if flags & FLAG_ATTESTED == 0 {
            return Err(Error::Format(
                "the authenticator data has no attested credential",
            ));
        }
        let counter = u32::from_be_bytes(data[33..37].try_into().unwrap());
        let aaguid: [u8; 16] = data[37..53].try_into().unwrap();
        let id_len = u16::from_be_bytes([data[53], data[54]]) as usize;
        let mut rest = &data[55..];
        if rest.len() < id_len {
            return Err(short);
        }
        let credential_id = rest[..id_len].to_vec();
        rest = &rest[id_len..];
        let cose_key: Value = ciborium::from_reader(&mut rest)
            .map_err(|_| Error::Format("the COSE key is not CBOR"))?;
        // Apple's sample appends the extensions map without setting the flag.
        let extensions = if flags & FLAG_EXTENSIONS != 0 || !rest.is_empty() {
            Some(
                ciborium::from_reader(&mut rest)
                    .map_err(|_| Error::Format("the extensions are not CBOR"))?,
            )
        } else {
            None
        };
        if !rest.is_empty() {
            return Err(Error::Format("trailing bytes after the authenticator data"));
        }
        Ok(Self {
            rp_id_hash: data[..32].to_vec(),
            counter,
            aaguid,
            credential_id,
            cose_key,
            extensions,
        })
    }

    /// The COSE EC2 P-256 key as an uncompressed point.
    fn cose_point(&self) -> Result<Vec<u8>, Error> {
        let map = self.cose_key.as_map().ok_or(Error::CoseKey)?;
        let field = |label: i64| {
            map.iter()
                .find(|(k, _)| {
                    k.as_integer()
                        .is_some_and(|i| i128::from(i) == label as i128)
                })
                .map(|(_, v)| v)
        };
        let int = |label| field(label).and_then(Value::as_integer).map(i128::from);
        // kty EC2, alg ES256, crv P-256.
        if int(1) != Some(2) || int(3) != Some(-7) || int(-1) != Some(1) {
            return Err(Error::CoseKey);
        }
        let coord = |label| {
            field(label)
                .and_then(Value::as_bytes)
                .filter(|b| b.len() == 32)
                .ok_or(Error::CoseKey)
        };
        let mut point = vec![4];
        point.extend_from_slice(coord(-2)?);
        point.extend_from_slice(coord(-3)?);
        Ok(point)
    }

    fn extensions(&self) -> Result<(Option<u32>, Option<String>), Error> {
        let Some(extensions) = &self.extensions else {
            return Ok((None, None));
        };
        let map = extensions
            .as_map()
            .ok_or(Error::Format("the extensions are not a map"))?;
        let find = |key: &str| {
            map.iter()
                .find(|(k, _)| text(k) == Some(key))
                .map(|(_, v)| v)
        };
        let category = match find("apple_validation_category_01") {
            None => None,
            // Apple's sample carries it as four little-endian bytes.
            Some(Value::Bytes(b)) if b.len() == 4 => {
                Some(u32::from_le_bytes(b[..].try_into().unwrap()))
            }
            Some(Value::Integer(i)) => Some(
                u32::try_from(i128::from(*i))
                    .map_err(|_| Error::Format("the validation category is out of range"))?,
            ),
            Some(_) => return Err(Error::Format("the validation category is not a UInt32")),
        };
        let version = match find("apple_bundle_version_01") {
            None => None,
            Some(Value::Text(t)) => Some(t.clone()),
            Some(_) => return Err(Error::Format("the bundle version is not a string")),
        };
        Ok((category, version))
    }
}

fn sha256(parts: &[&[u8]]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part);
    }
    hasher.finalize().to_vec()
}

#[cfg(test)]
mod tests;
