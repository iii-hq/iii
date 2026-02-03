use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use reqwest::{Client, Method};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    function::Function,
    invocation::{
        invoker::Invoker,
        method::{HttpAuth, HttpMethod, InvocationMethod},
        signature::sign_request,
        url_validator::{SecurityError, UrlValidator, UrlValidatorConfig},
    },
    protocol::ErrorBody,
};

pub struct HttpInvokerConfig {
    pub url_validator: UrlValidatorConfig,
    pub signature_max_age: u64,
    pub default_timeout_ms: u64,
    /// Maximum idle connections per host in the connection pool.
    /// Default: 50 connections per host.
    pub pool_max_idle_per_host: usize,
    /// Client-level timeout for all requests.
    /// This is the absolute maximum time for any request.
    /// Individual requests may have shorter timeouts via timeout_ms.
    /// Default: 60 seconds.
    pub client_timeout_secs: u64,
}

impl Default for HttpInvokerConfig {
    fn default() -> Self {
        Self {
            url_validator: UrlValidatorConfig::default(),
            signature_max_age: 300,
            default_timeout_ms: 30000,
            pool_max_idle_per_host: 50,
            client_timeout_secs: 60,
        }
    }
}

pub struct HttpInvoker {
    client: Client,
    url_validator: UrlValidator,
    signature_max_age: u64,
    default_timeout_ms: u64,
}

impl HttpInvoker {
    pub fn new(config: HttpInvokerConfig) -> Result<Self, SecurityError> {
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
        })
    }

    pub fn signature_max_age(&self) -> u64 {
        self.signature_max_age
    }

    pub fn url_validator(&self) -> &UrlValidator {
        &self.url_validator
    }

    async fn invoke_impl(
        &self,
        function: &Function,
        invocation_id: Uuid,
        data: Value,
        caller_function: Option<&str>,
        trace_id: Option<&str>,
    ) -> Result<Option<Value>, ErrorBody> {
        let InvocationMethod::Http {
            url,
            method,
            timeout_ms,
            headers,
            auth,
        } = &function.invocation_method
        else {
            return Err(ErrorBody {
                code: "invalid_invocation_method".into(),
                message: "Expected HTTP invocation method".into(),
            });
        };

        self.url_validator
            .validate(url)
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

        let body = serde_json::to_vec(&data).map_err(|err| ErrorBody {
            code: "serialization_error".into(),
            message: err.to_string(),
        })?;

        let timeout = timeout_ms.unwrap_or(self.default_timeout_ms);

        let mut request = self
            .client
            .request(http_method_to_reqwest(method), url)
            .timeout(Duration::from_millis(timeout))
            .header("Content-Type", "application/json")
            .header("x-iii-Function-Path", &function.function_path)
            .header("x-iii-Invocation-ID", invocation_id.to_string())
            .header("x-iii-Timestamp", timestamp.to_string());

        for (key, value) in headers {
            request = request.header(key, value);
        }

        if let Some(caller) = caller_function {
            request = request.header("x-iii-Caller-Function", caller);
        }
        if let Some(trace) = trace_id {
            request = request.header("x-iii-Trace-ID", trace);
        }

        request = match auth {
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

        let status = response.status();
        let bytes = response.bytes().await.map_err(|err| ErrorBody {
            code: "http_response_failed".into(),
            message: err.to_string(),
        })?;

        if status.is_success() {
            if bytes.is_empty() {
                return Ok(None);
            }
            let result: Value = serde_json::from_slice(&bytes).map_err(|err| ErrorBody {
                code: "invalid_response".into(),
                message: err.to_string(),
            })?;
            return Ok(Some(result));
        }

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
}

#[async_trait]
impl Invoker for HttpInvoker {
    fn method_type(&self) -> &'static str {
        "http"
    }

    async fn invoke(
        &self,
        function: &Function,
        invocation_id: Uuid,
        data: Value,
        caller_function: Option<&str>,
        trace_id: Option<&str>,
    ) -> Result<Option<Value>, ErrorBody> {
        self.invoke_impl(function, invocation_id, data, caller_function, trace_id)
            .await
    }
}

fn http_method_to_reqwest(method: &HttpMethod) -> Method {
    match method {
        HttpMethod::Post => Method::POST,
        HttpMethod::Put => Method::PUT,
    }
}
