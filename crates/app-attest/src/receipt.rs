//! The App Attest receipt: a PKCS #7 signed payload, as Apple's "Assessing
//! fraud risk" describes it. The one inside an attestation is of type
//! `ATTEST`; Apple's server answers with one of type `RECEIPT`, which carries
//! the risk metric.

use p256::ecdsa::{signature::Verifier as _, Signature, VerifyingKey};
use sha2::{Digest, Sha256};
use x509_parser::{certificate::X509Certificate, prelude::FromDer};

use crate::{check_chain, Error};

/// Apple Root CA - G3, from https://www.apple.com/certificateauthority/AppleRootCA-G3.cer.
/// Apple's sample receipt chains to it, not to the App Attest root.
const APPLE_ROOT_G3_PEM: &[u8] = include_bytes!("apple-root-ca-g3.pem");

/// The common name of the certificate that signed Apple's sample receipt.
const SIGNER_NAME: &str = "Application Attestation Fraud Receipt Signing";

const OID_SIGNED_DATA: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x07, 0x02];
const OID_DATA: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x07, 0x01];
const OID_MESSAGE_DIGEST: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x09, 0x04];
const OID_ECDSA_SHA256: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x02];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReceiptKind {
    /// Field 6 `ATTEST`: inside an attestation object, without the metric.
    Attest,
    /// Field 6 `RECEIPT`: from Apple's server, with the metric.
    Receipt,
}

/// A receipt whose signature and chain verified. Times are Unix seconds.
#[derive(Clone, Debug)]
pub struct Receipt {
    /// Field 2.
    pub app_id: String,
    /// Field 3: the credential certificate, DER.
    pub attested_certificate: Vec<u8>,
    /// Field 4.
    pub client_hash: Vec<u8>,
    /// Field 5.
    pub token: String,
    /// Field 6.
    pub kind: ReceiptKind,
    /// Field 7, not in Apple's table: `production` in Apple's sample.
    pub environment: Option<String>,
    /// Field 12.
    pub created: i64,
    /// Field 17, `RECEIPT` only.
    pub risk_metric: Option<u32>,
    /// Field 19.
    pub not_before: Option<i64>,
    /// Field 21.
    pub expires: Option<i64>,
}

impl Receipt {
    /// Whether field 3 is a certificate for `public_key` (an uncompressed point).
    pub fn attests(&self, public_key: &[u8]) -> bool {
        X509Certificate::from_der(&self.attested_certificate)
            .is_ok_and(|(_, cert)| cert.public_key().subject_public_key.data.as_ref() == public_key)
    }
}

/// Apple Root CA - G3, DER.
pub fn apple_root_g3() -> Vec<u8> {
    let (_, pem) =
        x509_parser::pem::parse_x509_pem(APPLE_ROOT_G3_PEM).expect("the bundled root is PEM");
    pem.contents
}

/// Steps 1 to 3 of "Verify the receipt": the signature, the chain to Apple
/// Root CA - G3 at `now`, and the payload. Steps 4 to 6 (App ID, age, key)
/// are the caller's, with the fields this returns.
pub fn verify_receipt(der: &[u8], now: i64) -> Result<Receipt, Error> {
    verify_receipt_with_root(der, now, &apple_root_g3())
}

/// The same against another root. Only tests pass anything but [`apple_root_g3`].
pub fn verify_receipt_with_root(der: &[u8], now: i64, root: &[u8]) -> Result<Receipt, Error> {
    let signed = SignedData::parse(der)?;
    let signer = signed.signer_chain()?;
    let leaf = check_chain(&signer, root, now)?;
    let named = leaf
        .subject()
        .iter_common_name()
        .any(|cn| cn.as_str() == Ok(SIGNER_NAME));
    if !named {
        return Err(Error::Receipt(
            "the signer is not Apple's receipt signing certificate",
        ));
    }
    let key = VerifyingKey::from_sec1_bytes(&leaf.public_key().subject_public_key.data)
        .map_err(|_| Error::Receipt("the signer's key is not P-256"))?;
    let signature = Signature::from_der(&signed.signature)
        .map_err(|_| Error::Receipt("the signature is not DER"))?;
    let message = match &signed.signed_attributes {
        None => signed.content.clone(),
        Some(attributes) => {
            if message_digest(attributes)? != Sha256::digest(&signed.content).as_slice() {
                return Err(Error::Receipt(
                    "the message digest attribute does not match",
                ));
            }
            // Signed as a SET, though the field is tagged [0].
            let mut set = attributes.clone();
            set[0] = 0x31;
            set
        }
    };
    key.verify(&message, &signature)
        .map_err(|_| Error::Receipt("the signature does not verify"))?;
    parse_payload(&signed.content)
}

struct SignedData {
    content: Vec<u8>,
    certificates: Vec<Vec<u8>>,
    signer_serial: Vec<u8>,
    signed_attributes: Option<Vec<u8>>,
    signature: Vec<u8>,
}

impl SignedData {
    fn parse(der: &[u8]) -> Result<Self, Error> {
        let bad = |what| Error::Receipt(what);
        let (info, rest) = Tlv::parse(der)?;
        if info.tag != 0x30 || !rest.is_empty() {
            return Err(bad("not a single ContentInfo"));
        }
        let info = info.children()?;
        if info.len() != 2 || !info[0].is_oid(OID_SIGNED_DATA) || info[1].tag != 0xa0 {
            return Err(bad("not PKCS #7 signed data"));
        }
        let signed = info[1].children()?;
        let signed = match signed.as_slice() {
            [one] if one.tag == 0x30 => one.children()?,
            _ => return Err(bad("signed data is not one sequence")),
        };
        let mut fields = signed.iter();
        let _version = fields.next().ok_or(bad("no version"))?;
        let _digests = fields
            .next()
            .filter(|t| t.tag == 0x31)
            .ok_or(bad("no digest algorithms"))?;
        let encapsulated = fields
            .next()
            .filter(|t| t.tag == 0x30)
            .ok_or(bad("no content"))?;
        let encapsulated = encapsulated.children()?;
        if encapsulated.len() != 2
            || !encapsulated[0].is_oid(OID_DATA)
            || encapsulated[1].tag != 0xa0
        {
            return Err(bad("the content is not data"));
        }
        let content = match encapsulated[1].children()?.as_slice() {
            [octets] => octets.octets()?,
            _ => return Err(bad("the content is not one octet string")),
        };
        let mut next = fields.next().ok_or(bad("no signer infos"))?;
        let mut certificates = Vec::new();
        if next.tag == 0xa0 {
            certificates = next.children()?.iter().map(|c| c.raw.to_vec()).collect();
            next = fields.next().ok_or(bad("no signer infos"))?;
        }
        if next.tag == 0xa1 {
            next = fields.next().ok_or(bad("no signer infos"))?;
        }
        if next.tag != 0x31 || fields.next().is_some() {
            return Err(bad("signer infos are not the last field"));
        }
        let signers = next.children()?;
        let signer = match signers.as_slice() {
            [one] if one.tag == 0x30 => one.children()?,
            _ => return Err(bad("not exactly one signer")),
        };
        let mut fields = signer.iter();
        let _version = fields.next().ok_or(bad("no signer version"))?;
        let sid = fields
            .next()
            .filter(|t| t.tag == 0x30)
            .ok_or(bad("no issuer and serial"))?;
        let signer_serial = match sid.children()?.as_slice() {
            [_, serial] if serial.tag == 0x02 => serial.content.to_vec(),
            _ => return Err(bad("the signer id is not issuer and serial")),
        };
        let _digest = fields.next().ok_or(bad("no signer digest algorithm"))?;
        let mut next = fields.next().ok_or(bad("no signature algorithm"))?;
        let mut signed_attributes = None;
        if next.tag == 0xa0 {
            signed_attributes = Some(next.raw.to_vec());
            next = fields.next().ok_or(bad("no signature algorithm"))?;
        }
        let algorithm = next.children()?;
        if !algorithm
            .first()
            .is_some_and(|oid| oid.is_oid(OID_ECDSA_SHA256))
        {
            return Err(bad("the signature is not ECDSA with SHA-256"));
        }
        let signature = fields
            .next()
            .filter(|t| t.tag == 0x04)
            .ok_or(bad("no signature"))?;
        Ok(Self {
            content,
            certificates,
            signer_serial,
            signed_attributes,
            signature: signature.content.to_vec(),
        })
    }

    /// The signer's certificate, then its issuers among the included ones.
    /// The included root is left out: the caller pins its own.
    fn signer_chain(&self) -> Result<Vec<Vec<u8>>, Error> {
        let parsed: Vec<(&Vec<u8>, X509Certificate)> = self
            .certificates
            .iter()
            .filter_map(|der| {
                X509Certificate::from_der(der)
                    .ok()
                    .map(|(_, cert)| (der, cert))
            })
            .collect();
        let mut current = parsed
            .iter()
            .find(|(_, cert)| cert.raw_serial() == self.signer_serial.as_slice())
            .ok_or(Error::Receipt("no included certificate is the signer's"))?;
        let mut chain = vec![current.0.clone()];
        while let Some(issuer) = parsed.iter().find(|(_, cert)| {
            cert.subject().as_raw() == current.1.issuer().as_raw()
                && cert.subject().as_raw() != cert.issuer().as_raw()
        }) {
            if chain.len() > 4 {
                return Err(Error::Receipt("the included chain loops"));
            }
            chain.push(issuer.0.clone());
            current = issuer;
        }
        Ok(chain)
    }
}

/// The `messageDigest` attribute among the signed attributes.
fn message_digest(attributes: &[u8]) -> Result<Vec<u8>, Error> {
    let (tlv, _) = Tlv::parse(attributes)?;
    for attribute in tlv.children()? {
        let parts = attribute.children()?;
        if parts.len() == 2 && parts[0].is_oid(OID_MESSAGE_DIGEST) {
            if let [value] = parts[1].children()?.as_slice() {
                return value.octets();
            }
        }
    }
    Err(Error::Receipt(
        "the signed attributes have no message digest",
    ))
}

/// SET OF SEQUENCE { type INTEGER, version INTEGER, value OCTET STRING }.
fn parse_payload(content: &[u8]) -> Result<Receipt, Error> {
    let bad = |what| Error::Receipt(what);
    let (set, rest) = Tlv::parse(content)?;
    if set.tag != 0x31 || !rest.is_empty() {
        return Err(bad("the payload is not a set"));
    }
    let mut fields = std::collections::BTreeMap::new();
    for field in set.children()? {
        let parts = field.children()?;
        let [kind, _version, value] = parts.as_slice() else {
            return Err(bad("a payload field is not type, version and value"));
        };
        if kind.tag != 0x02 || kind.content.len() > 4 || value.tag != 0x04 {
            return Err(bad("a payload field is malformed"));
        }
        let kind = kind
            .content
            .iter()
            .fold(0u32, |acc, b| acc << 8 | *b as u32);
        fields.insert(kind, value.content.to_vec());
    }
    let text = |field: u32| -> Result<Option<String>, Error> {
        fields
            .get(&field)
            .map(|v| String::from_utf8(v.clone()).map_err(|_| bad("a text field is not UTF-8")))
            .transpose()
    };
    let required = |field| text(field)?.ok_or(bad("a required field is missing"));
    let time = |field: u32| -> Result<Option<i64>, Error> {
        text(field)?
            .map(|t| parse_time(&t).ok_or(bad("a time field is malformed")))
            .transpose()
    };
    let kind = match required(6)?.as_str() {
        "ATTEST" => ReceiptKind::Attest,
        "RECEIPT" => ReceiptKind::Receipt,
        _ => return Err(bad("the receipt type is neither ATTEST nor RECEIPT")),
    };
    let risk_metric = text(17)?
        .map(|m| {
            m.trim()
                .parse()
                .map_err(|_| bad("the risk metric is not a number"))
        })
        .transpose()?;
    Ok(Receipt {
        app_id: required(2)?,
        attested_certificate: fields.get(&3).cloned().ok_or(bad("no attested key"))?,
        client_hash: fields.get(&4).cloned().unwrap_or_default(),
        token: text(5)?.unwrap_or_default(),
        kind,
        environment: text(7)?,
        created: time(12)?.ok_or(bad("no creation time"))?,
        risk_metric,
        not_before: time(19)?,
        expires: time(21)?,
    })
}

/// `YYYY-MM-DDTHH:MM:SS[.fff]Z` as Unix seconds.
fn parse_time(text: &str) -> Option<i64> {
    let text = text.strip_suffix('Z')?;
    let (date, clock) = text.split_once('T')?;
    let clock = clock.split('.').next()?;
    let number = |s: &str| s.parse::<i64>().ok();
    let date: Vec<i64> = date.split('-').map(number).collect::<Option<_>>()?;
    let clock: Vec<i64> = clock.split(':').map(number).collect::<Option<_>>()?;
    let ([y, m, d], [hh, mm, ss]) = (date.as_slice(), clock.as_slice()) else {
        return None;
    };
    let (y, m, d) = (*y, *m, *d);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || *hh > 23 || *mm > 59 || *ss > 60 {
        return None;
    }
    // Days from 1970-01-01 to y-m-d in the proleptic Gregorian calendar.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + hh * 3600 + mm * 60 + ss)
}

/// One BER element. Indefinite lengths are read up to their end-of-contents.
struct Tlv<'a> {
    tag: u8,
    /// The whole element.
    raw: &'a [u8],
    content: &'a [u8],
}

impl<'a> Tlv<'a> {
    fn parse(input: &'a [u8]) -> Result<(Self, &'a [u8]), Error> {
        Self::parse_at(input, 0)
    }

    fn parse_at(input: &'a [u8], depth: usize) -> Result<(Self, &'a [u8]), Error> {
        let short = Error::Receipt("a BER element is truncated");
        if depth > 32 {
            return Err(Error::Receipt("BER nests too deep"));
        }
        let (&tag, rest) = input.split_first().ok_or(short.clone())?;
        if tag & 0x1f == 0x1f {
            return Err(Error::Receipt("multi-byte BER tags are not used here"));
        }
        let (&first, rest) = rest.split_first().ok_or(short.clone())?;
        let header = input.len() - rest.len();
        if first == 0x80 {
            if tag & 0x20 == 0 {
                return Err(Error::Receipt(
                    "a primitive element has an indefinite length",
                ));
            }
            let mut cursor = rest;
            loop {
                if cursor.starts_with(&[0, 0]) {
                    let used = rest.len() - cursor.len();
                    let element = Tlv {
                        tag,
                        raw: &input[..header + used + 2],
                        content: &rest[..used],
                    };
                    return Ok((element, &cursor[2..]));
                }
                let (_, next) = Self::parse_at(cursor, depth + 1)?;
                cursor = next;
            }
        }
        let (len, rest) = if first < 0x80 {
            (first as usize, rest)
        } else {
            let count = (first & 0x7f) as usize;
            if count > 4 || rest.len() < count {
                return Err(short);
            }
            let len = rest[..count]
                .iter()
                .fold(0usize, |acc, b| acc << 8 | *b as usize);
            (len, &rest[count..])
        };
        if rest.len() < len {
            return Err(short);
        }
        let header = input.len() - rest.len();
        Ok((
            Tlv {
                tag,
                raw: &input[..header + len],
                content: &rest[..len],
            },
            &rest[len..],
        ))
    }

    fn children(&self) -> Result<Vec<Tlv<'a>>, Error> {
        let mut rest = self.content;
        let mut children = Vec::new();
        while !rest.is_empty() {
            let (child, next) = Self::parse_at(rest, 1)?;
            children.push(child);
            rest = next;
        }
        Ok(children)
    }

    fn is_oid(&self, oid: &[u8]) -> bool {
        self.tag == 0x06 && self.content == oid
    }

    /// An OCTET STRING's bytes, joining the chunks of a constructed one.
    fn octets(&self) -> Result<Vec<u8>, Error> {
        match self.tag {
            0x04 => Ok(self.content.to_vec()),
            0x24 => {
                let mut joined = Vec::new();
                for chunk in self.children()? {
                    joined.extend(chunk.octets()?);
                }
                Ok(joined)
            }
            _ => Err(Error::Receipt("not an octet string")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Forge, Options, ReceiptOptions};

    const NOW: i64 = 1_793_836_800;

    fn forged() -> (Forge, crate::testing::Forged) {
        let forge = Forge::new();
        let attested = forge.attest(&Options::new("678K8MRCKG.io.example", &[1; 32]));
        (forge, attested)
    }

    #[test]
    fn verifies_a_forged_receipt() {
        let (forge, attested) = forged();
        let der = forge.receipt(&ReceiptOptions::new(
            "678K8MRCKG.io.example",
            &attested.credential_certificate,
            NOW - 10,
        ));
        let receipt = verify_receipt_with_root(&der, NOW, forge.root()).unwrap();
        assert_eq!(receipt.kind, ReceiptKind::Receipt);
        assert_eq!(receipt.app_id, "678K8MRCKG.io.example");
        assert_eq!(receipt.risk_metric, Some(1));
        assert_eq!(receipt.created, NOW - 10);
        assert_eq!(receipt.environment.as_deref(), Some("sandbox"));
        let (_, cert) = X509Certificate::from_der(&attested.credential_certificate).unwrap();
        assert!(receipt.attests(&cert.public_key().subject_public_key.data));
        assert!(!receipt.attests(&[4; 65]));
    }

    #[test]
    fn the_attestation_carries_an_attest_receipt() {
        let (forge, attested) = forged();
        let attestation = crate::verify_with_root(
            &attested.object,
            &crate::Expected {
                app_id: "678K8MRCKG.io.example",
                key_id: &attested.key_id,
                client_data_hash: &[1; 32],
            },
            NOW,
            forge.root(),
        )
        .unwrap();
        let receipt = verify_receipt_with_root(&attestation.receipt, NOW, forge.root()).unwrap();
        assert_eq!(receipt.kind, ReceiptKind::Attest);
        assert_eq!(receipt.risk_metric, None);
        assert!(receipt.attests(&attestation.public_key));
    }

    #[test]
    fn refuses_tampered_misnamed_and_foreign_receipts() {
        let (forge, attested) = forged();
        let options = ReceiptOptions::new(
            "678K8MRCKG.io.example",
            &attested.credential_certificate,
            NOW,
        );
        let refused = |options: &ReceiptOptions, root: &[u8]| {
            verify_receipt_with_root(&forge.receipt(options), NOW, root).unwrap_err()
        };

        let mut tampered = options.clone();
        tampered.tampered = true;
        assert_eq!(
            refused(&tampered, forge.root()),
            Error::Receipt("the signature does not verify")
        );

        let mut misnamed = options.clone();
        misnamed.signer_name = "Someone Else".into();
        assert_eq!(
            refused(&misnamed, forge.root()),
            Error::Receipt("the signer is not Apple's receipt signing certificate")
        );

        assert!(matches!(
            refused(&options, Forge::new().root()),
            Error::Chain(_)
        ));
        assert!(matches!(
            refused(&options, &apple_root_g3()),
            Error::Chain(_)
        ));

        let mut other = options.clone();
        other.kind = "OTHER".into();
        assert!(matches!(refused(&other, forge.root()), Error::Receipt(_)));
        assert!(verify_receipt_with_root(b"not a receipt", NOW, forge.root()).is_err());
    }

    #[test]
    fn parses_times() {
        assert_eq!(parse_time("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_time("2026-04-21T18:13:12.153Z"), Some(1_776_795_192));
        assert_eq!(parse_time("2024-02-29T00:00:00Z"), Some(1_709_164_800));
        assert_eq!(parse_time("2026-13-01T00:00:00Z"), None);
        assert_eq!(parse_time("2026-04-21 18:13:12Z"), None);
    }

    #[test]
    fn reads_indefinite_lengths_and_chunked_octets() {
        // OCTET STRING, constructed, indefinite: "ab" + "c".
        let ber = [
            0x24, 0x80, 0x04, 0x02, b'a', b'b', 0x04, 0x01, b'c', 0, 0, 0xff,
        ];
        let (tlv, rest) = Tlv::parse(&ber).unwrap();
        assert_eq!(tlv.octets().unwrap(), b"abc");
        assert_eq!(rest, [0xff]);
        assert_eq!(tlv.raw.len(), 11);
        assert!(Tlv::parse(&ber[..8]).is_err());
        assert!(Tlv::parse(&[0x04, 0x80, 0, 0]).is_err());
    }
}
