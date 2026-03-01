// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use serde::{Deserialize, Serialize};

use crate::{invocation::method::HttpAuth, protocol::ErrorBody};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum HttpAuthConfig {
    Hmac {
        secret_key: String,
    },
    Bearer {
        token_key: String,
    },
    #[serde(rename = "api_key")]
    ApiKey {
        header: String,
        value_key: String,
    },
}

impl HttpAuthConfig {
    /// Validates that the environment variables referenced by this auth configuration exist,
    /// without resolving their values.
    pub fn validate(&self) -> Result<(), ErrorBody> {
        match self {
            HttpAuthConfig::Hmac { secret_key } => {
                std::env::var(secret_key).map_err(|_| ErrorBody {
                    code: "missing_env_var".into(),
                    message: format!(
                        "Missing environment variable '{}' for HMAC authentication. \
                         Please set this environment variable before registering the function.",
                        secret_key
                    ),
                })?;
            }
            HttpAuthConfig::Bearer { token_key } => {
                std::env::var(token_key).map_err(|_| ErrorBody {
                    code: "missing_env_var".into(),
                    message: format!(
                        "Missing environment variable '{}' for Bearer token authentication. \
                         Please set this environment variable before registering the function.",
                        token_key
                    ),
                })?;
            }
            HttpAuthConfig::ApiKey { value_key, .. } => {
                std::env::var(value_key).map_err(|_| ErrorBody {
                    code: "missing_env_var".into(),
                    message: format!(
                        "Missing environment variable '{}' for API key authentication. \
                         Please set this environment variable before registering the function.",
                        value_key
                    ),
                })?;
            }
        }
        Ok(())
    }
}

pub fn resolve_auth_ref(auth_ref: &HttpAuthConfig) -> Result<HttpAuth, ErrorBody> {
    match auth_ref {
        HttpAuthConfig::Hmac { secret_key } => {
            let secret = std::env::var(secret_key).map_err(|_| ErrorBody {
                code: "secret_not_found".into(),
                message: format!("Secret not found: {}", secret_key),
            })?;
            Ok(HttpAuth::Hmac { secret })
        }
        HttpAuthConfig::Bearer { token_key } => {
            let token = std::env::var(token_key).map_err(|_| ErrorBody {
                code: "token_not_found".into(),
                message: format!("Token not found: {}", token_key),
            })?;
            Ok(HttpAuth::Bearer { token })
        }
        HttpAuthConfig::ApiKey { header, value_key } => {
            let value = std::env::var(value_key).map_err(|_| ErrorBody {
                code: "api_key_not_found".into(),
                message: format!("API key not found: {}", value_key),
            })?;
            Ok(HttpAuth::ApiKey {
                header: header.clone(),
                value,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    // ---- HttpAuthConfig::validate ----

    #[test]
    #[serial]
    fn test_validate_hmac_success() {
        // SAFETY: Tests run serially via #[serial], no concurrent env access.
        unsafe { std::env::set_var("TEST_HMAC_SECRET", "supersecret") };
        let config = HttpAuthConfig::Hmac {
            secret_key: "TEST_HMAC_SECRET".to_string(),
        };
        assert!(config.validate().is_ok());
        unsafe { std::env::remove_var("TEST_HMAC_SECRET") };
    }

    #[test]
    #[serial]
    fn test_validate_hmac_missing_env() {
        unsafe { std::env::remove_var("TEST_HMAC_MISSING") };
        let config = HttpAuthConfig::Hmac {
            secret_key: "TEST_HMAC_MISSING".to_string(),
        };
        let result = config.validate();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code, "missing_env_var");
        assert!(err.message.contains("TEST_HMAC_MISSING"));
    }

    #[test]
    #[serial]
    fn test_validate_bearer_success() {
        unsafe { std::env::set_var("TEST_BEARER_TOKEN", "mytoken") };
        let config = HttpAuthConfig::Bearer {
            token_key: "TEST_BEARER_TOKEN".to_string(),
        };
        assert!(config.validate().is_ok());
        unsafe { std::env::remove_var("TEST_BEARER_TOKEN") };
    }

    #[test]
    #[serial]
    fn test_validate_bearer_missing_env() {
        unsafe { std::env::remove_var("TEST_BEARER_MISSING") };
        let config = HttpAuthConfig::Bearer {
            token_key: "TEST_BEARER_MISSING".to_string(),
        };
        let result = config.validate();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code, "missing_env_var");
        assert!(err.message.contains("TEST_BEARER_MISSING"));
    }

    #[test]
    #[serial]
    fn test_validate_api_key_success() {
        unsafe { std::env::set_var("TEST_API_KEY_VALUE", "key123") };
        let config = HttpAuthConfig::ApiKey {
            header: "X-Api-Key".to_string(),
            value_key: "TEST_API_KEY_VALUE".to_string(),
        };
        assert!(config.validate().is_ok());
        unsafe { std::env::remove_var("TEST_API_KEY_VALUE") };
    }

    #[test]
    #[serial]
    fn test_validate_api_key_missing_env() {
        unsafe { std::env::remove_var("TEST_API_KEY_MISSING") };
        let config = HttpAuthConfig::ApiKey {
            header: "X-Api-Key".to_string(),
            value_key: "TEST_API_KEY_MISSING".to_string(),
        };
        let result = config.validate();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code, "missing_env_var");
        assert!(err.message.contains("TEST_API_KEY_MISSING"));
    }

    // ---- resolve_auth_ref ----

    #[test]
    #[serial]
    fn test_resolve_auth_ref_hmac() {
        unsafe { std::env::set_var("TEST_RESOLVE_HMAC", "secret_value") };
        let config = HttpAuthConfig::Hmac {
            secret_key: "TEST_RESOLVE_HMAC".to_string(),
        };
        let result = resolve_auth_ref(&config).unwrap();
        match result {
            HttpAuth::Hmac { secret } => assert_eq!(secret, "secret_value"),
            _ => panic!("Expected HttpAuth::Hmac"),
        }
        unsafe { std::env::remove_var("TEST_RESOLVE_HMAC") };
    }

    #[test]
    #[serial]
    fn test_resolve_auth_ref_hmac_missing() {
        unsafe { std::env::remove_var("TEST_RESOLVE_HMAC_MISSING") };
        let config = HttpAuthConfig::Hmac {
            secret_key: "TEST_RESOLVE_HMAC_MISSING".to_string(),
        };
        let result = resolve_auth_ref(&config);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, "secret_not_found");
    }

    #[test]
    #[serial]
    fn test_resolve_auth_ref_bearer() {
        unsafe { std::env::set_var("TEST_RESOLVE_BEARER", "my_token") };
        let config = HttpAuthConfig::Bearer {
            token_key: "TEST_RESOLVE_BEARER".to_string(),
        };
        let result = resolve_auth_ref(&config).unwrap();
        match result {
            HttpAuth::Bearer { token } => assert_eq!(token, "my_token"),
            _ => panic!("Expected HttpAuth::Bearer"),
        }
        unsafe { std::env::remove_var("TEST_RESOLVE_BEARER") };
    }

    #[test]
    #[serial]
    fn test_resolve_auth_ref_bearer_missing() {
        unsafe { std::env::remove_var("TEST_RESOLVE_BEARER_MISSING") };
        let config = HttpAuthConfig::Bearer {
            token_key: "TEST_RESOLVE_BEARER_MISSING".to_string(),
        };
        let result = resolve_auth_ref(&config);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, "token_not_found");
    }

    #[test]
    #[serial]
    fn test_resolve_auth_ref_api_key() {
        unsafe { std::env::set_var("TEST_RESOLVE_APIKEY", "key_value_123") };
        let config = HttpAuthConfig::ApiKey {
            header: "X-Custom-Header".to_string(),
            value_key: "TEST_RESOLVE_APIKEY".to_string(),
        };
        let result = resolve_auth_ref(&config).unwrap();
        match result {
            HttpAuth::ApiKey { header, value } => {
                assert_eq!(header, "X-Custom-Header");
                assert_eq!(value, "key_value_123");
            }
            _ => panic!("Expected HttpAuth::ApiKey"),
        }
        unsafe { std::env::remove_var("TEST_RESOLVE_APIKEY") };
    }

    #[test]
    #[serial]
    fn test_resolve_auth_ref_api_key_missing() {
        unsafe { std::env::remove_var("TEST_RESOLVE_APIKEY_MISSING") };
        let config = HttpAuthConfig::ApiKey {
            header: "X-Custom-Header".to_string(),
            value_key: "TEST_RESOLVE_APIKEY_MISSING".to_string(),
        };
        let result = resolve_auth_ref(&config);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, "api_key_not_found");
    }

    // ---- Serialization ----

    #[test]
    fn test_http_auth_config_serialize_hmac() {
        let config = HttpAuthConfig::Hmac {
            secret_key: "MY_SECRET".to_string(),
        };
        let json = serde_json::to_value(&config).unwrap();
        assert_eq!(json["type"], "hmac");
        assert_eq!(json["secret_key"], "MY_SECRET");
    }

    #[test]
    fn test_http_auth_config_serialize_bearer() {
        let config = HttpAuthConfig::Bearer {
            token_key: "MY_TOKEN".to_string(),
        };
        let json = serde_json::to_value(&config).unwrap();
        assert_eq!(json["type"], "bearer");
        assert_eq!(json["token_key"], "MY_TOKEN");
    }

    #[test]
    fn test_http_auth_config_serialize_api_key() {
        let config = HttpAuthConfig::ApiKey {
            header: "X-Api-Key".to_string(),
            value_key: "KEY_VAR".to_string(),
        };
        let json = serde_json::to_value(&config).unwrap();
        assert_eq!(json["type"], "api_key");
        assert_eq!(json["header"], "X-Api-Key");
        assert_eq!(json["value_key"], "KEY_VAR");
    }

    #[test]
    fn test_http_auth_config_deserialize_hmac() {
        let json = r#"{"type": "hmac", "secret_key": "S"}"#;
        let config: HttpAuthConfig = serde_json::from_str(json).unwrap();
        assert!(matches!(config, HttpAuthConfig::Hmac { secret_key } if secret_key == "S"));
    }

    #[test]
    fn test_http_auth_config_deserialize_bearer() {
        let json = r#"{"type": "bearer", "token_key": "T"}"#;
        let config: HttpAuthConfig = serde_json::from_str(json).unwrap();
        assert!(matches!(config, HttpAuthConfig::Bearer { token_key } if token_key == "T"));
    }

    #[test]
    fn test_http_auth_config_deserialize_api_key() {
        let json = r#"{"type": "api_key", "header": "H", "value_key": "V"}"#;
        let config: HttpAuthConfig = serde_json::from_str(json).unwrap();
        match config {
            HttpAuthConfig::ApiKey { header, value_key } => {
                assert_eq!(header, "H");
                assert_eq!(value_key, "V");
            }
            _ => panic!("Expected ApiKey variant"),
        }
    }
}
