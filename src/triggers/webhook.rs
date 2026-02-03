use std::{sync::Arc, time::{Duration, SystemTime, UNIX_EPOCH}};

use reqwest::Client;
use serde_json::Value;
use uuid::Uuid;

use crate::{
    bridge_api::token::BridgeTokenRegistry,
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
    pub bridge_url: Option<String>,
    pub token_registry: Option<Arc<BridgeTokenRegistry>>,
    /// Maximum idle connections per host in the connection pool.
    /// Default: 50 connections per host.
    pub pool_max_idle_per_host: usize,
    /// Client-level timeout for all requests.
    /// This is the absolute maximum time for any request.
    /// Default: 60 seconds.
    pub client_timeout_secs: u64,
}

impl Default for WebhookConfig {
    fn default() -> Self {
        Self {
            url_validator: UrlValidatorConfig::default(),
            signature_max_age: 300,
            default_timeout_ms: 30000,
            bridge_url: None,
            token_registry: None,
            pool_max_idle_per_host: 50,
            client_timeout_secs: 60,
        }
    }
}

pub struct WebhookDispatcher {
    client: Client,
    url_validator: UrlValidator,
    signature_max_age: u64,
    default_timeout_ms: u64,
    bridge_url: Option<String>,
    token_registry: Option<Arc<BridgeTokenRegistry>>,
}

impl WebhookDispatcher {
    pub fn new(config: WebhookConfig) -> Result<Self, SecurityError> {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.client_timeout_secs))
            .pool_max_idle_per_host(config.pool_max_idle_per_host)
            .build()
            .map_err(|_| SecurityError::InvalidUrl)?;
        let url_validator = UrlValidator::new(config.url_validator)?;
        Ok(Self {
            client,
            url_validator,
            signature_max_age: config.signature_max_age,
            default_timeout_ms: config.default_timeout_ms,
            bridge_url: config.bridge_url,
            token_registry: config.token_registry,
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

        let invocation_id = Uuid::new_v4().to_string();
        let trace_id = format!("trace-{}", Uuid::new_v4());

        let bridge_token = if let (Some(registry), Some(_bridge_url)) =
            (&self.token_registry, &self.bridge_url)
        {
            Some(registry.create_token(&invocation_id, &trigger.function_path, &trace_id))
        } else {
            None
        };

        let mut request = self
            .client
            .post(&trigger.url)
            .timeout(Duration::from_millis(self.default_timeout_ms))
            .header("Content-Type", "application/json")
            .header("x-iii-Trigger-Type", &trigger.trigger_type)
            .header("x-iii-Trigger-ID", &trigger.trigger_id)
            .header("x-iii-Function-Path", &trigger.function_path)
            .header("x-iii-Timestamp", timestamp.to_string())
            .header("x-iii-Invocation-ID", &invocation_id)
            .header("x-iii-Trace-ID", &trace_id);

        if let (Some(token), Some(url)) = (&bridge_token, &self.bridge_url) {
            request = request
                .header("x-iii-Bridge-URL", url)
                .header("x-iii-Bridge-Token", token);
        }

        request = match trigger.auth.as_ref() {
            Some(HttpAuth::Hmac { secret }) => {
                let signature = sign_request(&body, secret, timestamp);
                request.header("x-iii-Signature", signature)
            }
            Some(HttpAuth::Bearer { token }) => request.bearer_auth(token),
            Some(HttpAuth::ApiKey { header, value }) => request.header(header, value),
            None => request,
        };

        let response = request.body(body).send().await.map_err(|err| ErrorBody {
            code: "http_request_failed".into(),
            message: err.to_string(),
        })?;

        if let (Some(token), Some(registry)) = (&bridge_token, &self.token_registry) {
            registry.invalidate_token(token);
        }

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

    pub fn token_registry(&self) -> Option<Arc<BridgeTokenRegistry>> {
        self.token_registry.clone()
    }
}
