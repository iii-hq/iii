//! HTTP client auto-instrumentation for the III Rust SDK.
//!
//! Provides [`execute_traced_request`] which wraps a `reqwest::Request` in an
//! OTel `CLIENT` span with HTTP semantic-convention attributes, matching the
//! Node.js `fetch-instrumentation.ts` behavior.
//!
//! Uses `opentelemetry-http`'s [`HeaderInjector`] for W3C traceparent injection.
//!
//! # Example
//! ```no_run
//! use iii_sdk::telemetry::http_instrumentation::execute_traced_request;
//!
//! # async fn example() -> Result<(), reqwest::Error> {
//! let client = reqwest::Client::new();
//! let request = client.get("https://example.com/api").build().unwrap();
//! let response = execute_traced_request(&client, request).await.unwrap();
//! # Ok(())
//! # }
//! ```

use opentelemetry::trace::{SpanKind, TraceContextExt, Tracer};
use opentelemetry::{Context as OtelContext, KeyValue};
use opentelemetry_http::HeaderInjector;
use reqwest::{Client, Request, Response};

const SAFE_REQUEST_HEADERS: &[&str] = &["content-type", "accept"];
const SAFE_RESPONSE_HEADERS: &[&str] = &["content-type"];

/// Build a span name matching the Node.js convention: `{METHOD} {path}` or `{METHOD}`.
fn span_name(method: &str, path: Option<&str>) -> String {
    match path {
        Some(p) if !p.is_empty() => format!("{} {}", method, p),
        _ => method.to_string(),
    }
}

/// Execute a `reqwest::Request` inside an OTel CLIENT span.
///
/// - Injects W3C traceparent into outgoing request headers via [`HeaderInjector`].
/// - Records HTTP semantic convention attributes on the span.
/// - Sets `ERROR` span status for HTTP responses with status >= 400.
/// - Records exceptions for network-level errors.
///
/// # Arguments
/// * `client` – `reqwest::Client` to send the request
/// * `request` – `reqwest::Request` to instrument (consumed)
pub async fn execute_traced_request(
    client: &Client,
    mut request: Request,
) -> Result<Response, reqwest::Error> {
    let url = request.url().clone();
    let method = request.method().as_str().to_uppercase();

    let host = url.host_str().map(String::from);
    let scheme = Some(url.scheme())
        .filter(|s| !s.is_empty())
        .map(String::from);
    let path = Some(url.path()).filter(|p| !p.is_empty()).map(String::from);
    let port = url.port();
    let query = url.query().map(String::from);

    let mut span_attrs: Vec<KeyValue> = vec![
        KeyValue::new("http.request.method", method.clone()),
        KeyValue::new("url.full", url.to_string()),
    ];
    if let Some(ref h) = host {
        span_attrs.push(KeyValue::new("server.address", h.clone()));
    }
    if let Some(ref s) = scheme {
        span_attrs.push(KeyValue::new("url.scheme", s.clone()));
        span_attrs.push(KeyValue::new("network.protocol.name", "http"));
    }
    if let Some(ref p) = path {
        span_attrs.push(KeyValue::new("url.path", p.clone()));
    }
    if let Some(p) = port {
        span_attrs.push(KeyValue::new("server.port", p as i64));
    }
    if let Some(ref q) = query {
        span_attrs.push(KeyValue::new("url.query", q.clone()));
    }

    let name = span_name(&method, path.as_deref());
    let tracer = opentelemetry::global::tracer("iii-rust-sdk");
    let cx = OtelContext::current();

    let span = tracer
        .span_builder(name)
        .with_kind(SpanKind::Client)
        .with_attributes(span_attrs)
        .start_with_context(&tracer, &cx);

    let cx = cx.with_span(span);

    // Inject W3C traceparent/tracestate using opentelemetry-http's HeaderInjector
    opentelemetry::global::get_text_map_propagator(|propagator| {
        propagator.inject_context(&cx, &mut HeaderInjector(request.headers_mut()));
    });

    // Capture safe request headers as span attributes
    for &name in SAFE_REQUEST_HEADERS {
        if let Some(value) = request.headers().get(name) {
            if let Ok(v) = value.to_str() {
                cx.span().set_attribute(KeyValue::new(
                    format!("http.request.header.{}", name),
                    v.to_string(),
                ));
            }
        }
    }

    // Capture request body size
    if let Some(body) = request.body() {
        if let Some(bytes) = body.as_bytes() {
            cx.span()
                .set_attribute(KeyValue::new("http.request.body.size", bytes.len() as i64));
        }
    }

    match client.execute(request).await {
        Ok(response) => {
            let status = response.status().as_u16();
            cx.span()
                .set_attribute(KeyValue::new("http.response.status_code", status as i64));

            if let Some(cl) = response.headers().get("content-length") {
                if let Ok(s) = cl.to_str() {
                    if let Ok(n) = s.parse::<i64>() {
                        cx.span()
                            .set_attribute(KeyValue::new("http.response.body.size", n));
                    }
                }
            }

            for &name in SAFE_RESPONSE_HEADERS {
                if let Some(value) = response.headers().get(name) {
                    if let Ok(v) = value.to_str() {
                        cx.span().set_attribute(KeyValue::new(
                            format!("http.response.header.{}", name),
                            v.to_string(),
                        ));
                    }
                }
            }

            if status >= 400 {
                cx.span()
                    .set_attribute(KeyValue::new("error.type", status.to_string()));
                cx.span()
                    .set_status(opentelemetry::trace::Status::error(status.to_string()));
            } else {
                cx.span().set_status(opentelemetry::trace::Status::Ok);
            }

            cx.span().end();
            Ok(response)
        }
        Err(err) => {
            cx.span()
                .set_attribute(KeyValue::new("error.type", err.to_string()));
            cx.span()
                .set_status(opentelemetry::trace::Status::error(err.to_string()));
            let backtrace = std::backtrace::Backtrace::force_capture();
            cx.span().add_event(
                "exception",
                vec![
                    KeyValue::new("exception.type", "reqwest::Error"),
                    KeyValue::new("exception.message", err.to_string()),
                    KeyValue::new("exception.stacktrace", backtrace.to_string()),
                ],
            );
            cx.span().end();
            Err(err)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_safe_request_headers_contains_content_type_and_accept() {
        assert!(SAFE_REQUEST_HEADERS.contains(&"content-type"));
        assert!(SAFE_REQUEST_HEADERS.contains(&"accept"));
    }

    #[test]
    fn test_safe_response_headers_contains_content_type() {
        assert!(SAFE_RESPONSE_HEADERS.contains(&"content-type"));
    }

    #[test]
    fn test_span_name_with_path() {
        assert_eq!(span_name("GET", Some("/api/items")), "GET /api/items");
        assert_eq!(span_name("POST", Some("/users")), "POST /users");
    }

    #[test]
    fn test_span_name_without_path() {
        assert_eq!(span_name("GET", None), "GET");
        assert_eq!(span_name("DELETE", Some("")), "DELETE");
    }
}
