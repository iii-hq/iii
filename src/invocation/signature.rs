use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use ring::hmac;
use subtle::ConstantTimeEq;

#[derive(Debug)]
pub enum SignatureError {
    SignatureExpired,
    InvalidSignature,
    ClockSkew,
}

impl std::fmt::Display for SignatureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SignatureError::SignatureExpired => write!(f, "Signature expired"),
            SignatureError::InvalidSignature => write!(f, "Invalid signature"),
            SignatureError::ClockSkew => write!(f, "Clock skew"),
        }
    }
}

impl std::error::Error for SignatureError {}

pub fn sign_request(body: &[u8], secret: &str, timestamp: u64) -> String {
    let body_b64 = base64::engine::general_purpose::STANDARD.encode(body);
    let payload = format!("{}:{}", timestamp, body_b64);
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
    let signature = hmac::sign(&key, payload.as_bytes());
    format!("sha256={}", hex::encode(signature.as_ref()))
}

pub fn verify_signature(
    body: &[u8],
    signature: &str,
    secret: &str,
    timestamp: u64,
    max_age: u64,
) -> Result<(), SignatureError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| SignatureError::ClockSkew)?
        .as_secs();

    if now.saturating_sub(timestamp) > max_age {
        return Err(SignatureError::SignatureExpired);
    }

    let expected = sign_request(body, secret, timestamp);
    if signature.as_bytes().ct_eq(expected.as_bytes()).into() {
        Ok(())
    } else {
        Err(SignatureError::InvalidSignature)
    }
}
