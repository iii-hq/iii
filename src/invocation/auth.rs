use serde::{Deserialize, Serialize};

use crate::{invocation::method::HttpAuth, protocol::ErrorBody};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum HttpAuthRef {
    Hmac { secret_key: String },
    Bearer { token_key: String },
    ApiKey { header: String, value_key: String },
}

pub fn resolve_auth_ref(auth_ref: &HttpAuthRef) -> Result<HttpAuth, ErrorBody> {
    match auth_ref {
        HttpAuthRef::Hmac { secret_key } => {
            let secret = std::env::var(secret_key).map_err(|_| ErrorBody {
                code: "secret_not_found".into(),
                message: format!("Secret not found: {}", secret_key),
            })?;
            Ok(HttpAuth::Hmac { secret })
        }
        HttpAuthRef::Bearer { token_key } => {
            let token = std::env::var(token_key).map_err(|_| ErrorBody {
                code: "token_not_found".into(),
                message: format!("Token not found: {}", token_key),
            })?;
            Ok(HttpAuth::Bearer { token })
        }
        HttpAuthRef::ApiKey { header, value_key } => {
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
