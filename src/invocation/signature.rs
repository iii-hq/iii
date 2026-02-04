use base64::Engine;
use ring::hmac;

pub fn sign_request(body: &[u8], secret: &str, timestamp: u64) -> String {
    let body_b64 = base64::engine::general_purpose::STANDARD.encode(body);
    let payload = format!("{}:{}", timestamp, body_b64);
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
    let signature = hmac::sign(&key, payload.as_bytes());
    format!("sha256={}", hex::encode(signature.as_ref()))
}
