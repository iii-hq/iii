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
