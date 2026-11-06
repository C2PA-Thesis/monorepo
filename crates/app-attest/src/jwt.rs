//! The token for Apple's `attestationData` endpoint. "Assessing fraud risk",
//! Create a request: "Generate the token using the same procedure you use to
//! create a provider authentication token for the Apple Push Notification
//! service (APNs)". That is ES256 with the key id in the header and the Team
//! ID and issue time in the claims.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use p256::{
    ecdsa::{signature::Signer as _, Signature, SigningKey},
    pkcs8::DecodePrivateKey,
};

/// A DeviceCheck key, from the `.p8` file Apple hands out once.
pub struct DeviceCheckKey {
    key: SigningKey,
    key_id: String,
    team_id: String,
}

impl DeviceCheckKey {
    pub fn from_p8(pem: &str, key_id: &str, team_id: &str) -> Result<Self, String> {
        let key = SigningKey::from_pkcs8_pem(pem.trim())
            .map_err(|_| "the DeviceCheck key is not a P-256 PKCS #8 PEM".to_string())?;
        let id_ok = |id: &str| id.len() == 10 && id.bytes().all(|b| b.is_ascii_alphanumeric());
        if !id_ok(key_id) {
            return Err("the DeviceCheck key id is not 10 letters and digits".into());
        }
        if !id_ok(team_id) {
            return Err("the Team ID is not 10 letters and digits".into());
        }
        Ok(Self {
            key,
            key_id: key_id.into(),
            team_id: team_id.into(),
        })
    }

    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    /// A token issued at `now` (Unix seconds).
    pub fn token(&self, now: i64) -> String {
        let header = format!(r#"{{"alg":"ES256","kid":"{}"}}"#, self.key_id);
        let claims = format!(r#"{{"iss":"{}","iat":{now}}}"#, self.team_id);
        let input = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(header),
            URL_SAFE_NO_PAD.encode(claims)
        );
        let signature: Signature = self.key.sign(input.as_bytes());
        format!("{input}.{}", URL_SAFE_NO_PAD.encode(signature.to_bytes()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use p256::{
        ecdsa::{signature::Verifier as _, VerifyingKey},
        pkcs8::{EncodePrivateKey, LineEnding},
    };

    fn p8() -> (String, VerifyingKey) {
        let key = SigningKey::from_slice(&[7u8; 32]).unwrap();
        let pem = key.to_pkcs8_pem(LineEnding::LF).unwrap().to_string();
        (pem, *key.verifying_key())
    }

    #[test]
    fn signs_an_es256_token() {
        let (pem, public) = p8();
        let key = DeviceCheckKey::from_p8(&pem, "ABC123DEFG", "678K8MRCKG").unwrap();
        let token = key.token(1_793_836_800);
        let parts: Vec<&str> = token.split('.').collect();
        assert_eq!(parts.len(), 3);
        let decode = |s: &str| URL_SAFE_NO_PAD.decode(s).unwrap();
        let header: serde_json::Value = serde_json::from_slice(&decode(parts[0])).unwrap();
        let claims: serde_json::Value = serde_json::from_slice(&decode(parts[1])).unwrap();
        assert_eq!(
            header,
            serde_json::json!({"alg": "ES256", "kid": "ABC123DEFG"})
        );
        assert_eq!(
            claims,
            serde_json::json!({"iss": "678K8MRCKG", "iat": 1_793_836_800})
        );
        let signature = Signature::from_slice(&decode(parts[2])).unwrap();
        let input = format!("{}.{}", parts[0], parts[1]);
        public.verify(input.as_bytes(), &signature).unwrap();
    }

    #[test]
    fn refuses_bad_keys_and_ids() {
        let (pem, _) = p8();
        assert!(DeviceCheckKey::from_p8("not a key", "ABC123DEFG", "678K8MRCKG").is_err());
        assert!(DeviceCheckKey::from_p8(&pem, "short", "678K8MRCKG").is_err());
        assert!(DeviceCheckKey::from_p8(&pem, "ABC123DEFG", "678K8MRCKG\"").is_err());
    }
}
