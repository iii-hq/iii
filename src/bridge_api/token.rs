use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use dashmap::DashMap;
use ring::hmac;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeTokenData {
    pub invocation_id: String,
    pub function_path: String,
    pub trace_id: String,
    pub created_at: u64,
}

pub struct BridgeTokenRegistry {
    active_tokens: DashMap<String, BridgeTokenData>,
    secret: String,
}

impl BridgeTokenRegistry {
    pub fn new(secret: String) -> Self {
        Self {
            active_tokens: DashMap::new(),
            secret,
        }
    }

    pub fn create_token(
        &self,
        invocation_id: &str,
        function_path: &str,
        trace_id: &str,
    ) -> String {
        let created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let data = BridgeTokenData {
            invocation_id: invocation_id.to_string(),
            function_path: function_path.to_string(),
            trace_id: trace_id.to_string(),
            created_at,
        };

        let token = self.sign_token(&data);
        self.active_tokens.insert(token.clone(), data);

        token
    }

    pub fn validate_token(&self, token: &str) -> Option<BridgeTokenData> {
        self.active_tokens.get(token).map(|r| r.value().clone())
    }

    pub fn invalidate_token(&self, token: &str) {
        self.active_tokens.remove(token);
    }

    pub fn cleanup_old_tokens(&self, max_age_seconds: u64) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        self.active_tokens.retain(|_, data| {
            now - data.created_at < max_age_seconds
        });
    }

    pub fn active_token_count(&self) -> usize {
        self.active_tokens.len()
    }

    fn sign_token(&self, data: &BridgeTokenData) -> String {
        let json_data = serde_json::to_string(data).unwrap();
        let payload = base64::engine::general_purpose::STANDARD.encode(&json_data);

        let key = hmac::Key::new(hmac::HMAC_SHA256, self.secret.as_bytes());
        let signature = hmac::sign(&key, payload.as_bytes());
        let signature_hex = hex::encode(signature.as_ref());

        format!("{}.{}", payload, signature_hex)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_and_validate_token() {
        let registry = BridgeTokenRegistry::new("test-secret".to_string());
        let token = registry.create_token("inv-123", "test.function", "trace-456");

        let data = registry.validate_token(&token);
        assert!(data.is_some());

        let data = data.unwrap();
        assert_eq!(data.invocation_id, "inv-123");
        assert_eq!(data.function_path, "test.function");
        assert_eq!(data.trace_id, "trace-456");
    }

    #[test]
    fn test_invalidate_token() {
        let registry = BridgeTokenRegistry::new("test-secret".to_string());
        let token = registry.create_token("inv-123", "test.function", "trace-456");

        assert!(registry.validate_token(&token).is_some());
        registry.invalidate_token(&token);
        assert!(registry.validate_token(&token).is_none());
    }

    #[test]
    fn test_cleanup_old_tokens() {
        let registry = BridgeTokenRegistry::new("test-secret".to_string());
        registry.create_token("inv-1", "test.function", "trace-1");
        registry.create_token("inv-2", "test.function", "trace-2");

        assert_eq!(registry.active_token_count(), 2);

        registry.cleanup_old_tokens(0);
        assert_eq!(registry.active_token_count(), 0);
    }
}
