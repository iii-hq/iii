use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::Client;
use serde_json::Value;

use crate::{
    invocation::{
        method::HttpAuth,
        signature::sign_request,
        url_validator::{SecurityError, UrlValidator, UrlValidatorConfig},
    },
    protocol::ErrorBody,
    triggers::http::HttpTrigger,
};

pub struct WebhookConfig {
    pub url_validator: UrlValidatorConfig,
    pub signature_max_age: u64,
    pub default_timeout_ms: u64,
}

impl Default for WebhookConfig {
    fn default() -> Self {
        Self {
            url_validator: UrlValidatorConfig::default(),
            signature_max_age: 300,
            default_timeout_ms: 30000,
        }
    }
}

pub struct WebhookDispatcher {
    client: Client,
    url_validator: UrlValidator,
    signature_max_age: u64,
    default_timeout_ms: u64,
}

impl WebhookDispatcher {
    pub fn new(config: WebhookConfig) -> Result<Self, SecurityError> {
        let client = Client::builder()
            .timeout(Duration::from_secs(60))
            .pool_max_idle_per_host(10)
            .build()
            .map_err(|_| SecurityError::InvalidUrl)?;
        let url_validator = UrlValidator::new(config.url_validator)?;
        Ok(Self {
            client,
            url_validator,
            signature_max_age: config.signature_max_age,
            default_timeout_ms: config.default_timeout_ms,
        })
    }

    pub async fn deliver(&self, trigger: &HttpTrigger, payload: Value) -> Result<(), ErrorBody> {
        self.url_validator
            .validate(&trigger.url)
            .await
            .map_err(|e| ErrorBody {
                code: "url_validation_failed".into(),
                message: e.to_string(),
            })?;

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|err| ErrorBody {
                code: "timestamp_error".into(),
                message: err.to_string(),
            })?
            .as_secs();

        let body = serde_json::to_vec(&payload).map_err(|err| ErrorBody {
            code: "serialization_error".into(),
            message: err.to_string(),
        })?;

        let mut request = self
            .client
            .post(&trigger.url)
            .timeout(Duration::from_millis(self.default_timeout_ms))
            .header("Content-Type", "application/json")
            .header("X-III-Trigger-Type", &trigger.trigger_type)
            .header("X-III-Trigger-ID", &trigger.trigger_id)
            .header("X-III-Function-Path", &trigger.function_path)
            .header("X-III-Timestamp", timestamp.to_string());

        request = match trigger.auth.as_ref() {
            Some(HttpAuth::Hmac { secret }) => {
                let signature = sign_request(&body, secret, timestamp);
                request.header("X-III-Signature", signature)
            }
            Some(HttpAuth::Bearer { token }) => request.bearer_auth(token),
            Some(HttpAuth::ApiKey { header, value }) => request.header(header, value),
            None => request,
        };

        let response = request.body(body).send().await.map_err(|err| ErrorBody {
            code: "http_request_failed".into(),
            message: err.to_string(),
        })?;

        if response.status().is_success() {
            return Ok(());
        }

        let status = response.status();
        let bytes = response.bytes().await.map_err(|err| ErrorBody {
            code: "http_response_failed".into(),
            message: err.to_string(),
        })?;

        let error_json: Option<Value> = serde_json::from_slice(&bytes).ok();
        if let Some(error_json) = error_json {
            if let Some(error_obj) = error_json.get("error") {
                let code = error_obj
                    .get("code")
                    .and_then(|v| v.as_str())
                    .unwrap_or("http_error")
                    .to_string();
                let message = error_obj
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("HTTP request failed")
                    .to_string();
                return Err(ErrorBody { code, message });
            }
        }

        Err(ErrorBody {
            code: "http_error".into(),
            message: format!("HTTP {}", status),
        })
    }

    pub fn signature_max_age(&self) -> u64 {
        self.signature_max_age
    }

    pub fn url_validator(&self) -> &UrlValidator {
        &self.url_validator
    }
}
