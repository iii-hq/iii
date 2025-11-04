use std::{collections::HashMap, net::SocketAddr, sync::Arc, time::Duration};

use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path, State},
    http::{Method, StatusCode},
    response::{IntoResponse, Response as AxumResponse},
    routing::{any, get},
};
use prost_types::{Value, value::Kind as ValueKind};
use serde::Serialize;
use serde_json::{Map as JsonMap, Number as JsonNumber, Value as JsonValue};
use tokio::{net::TcpListener, sync::RwLock, time::timeout};
use tonic::{Code, Request, Status};
use tracing::info;

use crate::{MethodKind, ProcessRequest, RegisteredService, engine::worker_client::WorkerClient};

#[derive(Clone)]
pub struct HttpMapping {
    pub method: String,
    pub path: String,
}

#[derive(Clone)]
pub struct HttpRoute {
    pub service: String,
    pub method: String,
    pub http_method: String,
    pub path: String,
}

#[derive(Clone)]
pub struct HttpState {
    pub registry: Arc<RwLock<HashMap<String, RegisteredService>>>,
    pub routes: Arc<RwLock<HashMap<String, HttpRoute>>>,
    pub request_timeout: Duration,
}

#[derive(Serialize)]
struct ApiResponse {
    result: String,
}

#[derive(Serialize)]
struct ApiRouteEntry {
    service: String,
    method: String,
    http_method: String,
    path: String,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, message)
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, message)
    }

    fn method_not_allowed(message: impl Into<String>) -> Self {
        Self::new(StatusCode::METHOD_NOT_ALLOWED, message)
    }

    fn gateway_timeout(message: impl Into<String>) -> Self {
        Self::new(StatusCode::GATEWAY_TIMEOUT, message)
    }

    fn upstream(status: Status) -> Self {
        let status_code = match status.code() {
            Code::InvalidArgument => StatusCode::BAD_REQUEST,
            Code::Unauthenticated => StatusCode::UNAUTHORIZED,
            Code::PermissionDenied => StatusCode::FORBIDDEN,
            Code::NotFound => StatusCode::BAD_GATEWAY,
            Code::AlreadyExists => StatusCode::BAD_GATEWAY,
            Code::FailedPrecondition => StatusCode::BAD_GATEWAY,
            Code::Aborted => StatusCode::BAD_GATEWAY,
            Code::OutOfRange => StatusCode::BAD_REQUEST,
            Code::Unimplemented => StatusCode::BAD_GATEWAY,
            Code::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
            Code::DataLoss => StatusCode::BAD_GATEWAY,
            Code::DeadlineExceeded => StatusCode::GATEWAY_TIMEOUT,
            _ => StatusCode::BAD_GATEWAY,
        };

        let message = status.message().to_string();
        let fallback = status.code().to_string();
        Self::new(
            status_code,
            if message.is_empty() {
                fallback
            } else {
                message
            },
        )
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> AxumResponse {
        let body = Json(ErrorResponse {
            error: self.message.clone(),
        });
        (self.status, body).into_response()
    }
}

pub fn make_route_key(method: &str, path: &str) -> String {
    format!("{} {}", method.to_uppercase(), path)
}

fn value_to_json(value: &Value) -> JsonValue {
    match value.kind.as_ref() {
        Some(ValueKind::NullValue(_)) | None => JsonValue::Null,
        Some(ValueKind::BoolValue(v)) => JsonValue::Bool(*v),
        Some(ValueKind::NumberValue(v)) => JsonNumber::from_f64(*v)
            .map(JsonValue::Number)
            .unwrap_or_else(|| JsonValue::String(v.to_string())),
        Some(ValueKind::StringValue(v)) => JsonValue::String(v.clone()),
        Some(ValueKind::ListValue(list)) => JsonValue::Array(
            list.values
                .iter()
                .map(value_to_json)
                .collect::<Vec<JsonValue>>(),
        ),
        Some(ValueKind::StructValue(st)) => {
            let mut map = JsonMap::new();
            for (key, val) in &st.fields {
                map.insert(key.clone(), value_to_json(val));
            }
            JsonValue::Object(map)
        }
    }
}

pub fn parse_http_mapping(request_format: &Option<Value>) -> Result<Option<HttpMapping>, Status> {
    let Some(value) = request_format.as_ref() else {
        return Ok(None);
    };

    let json = value_to_json(value);
    let Some(http_value) = json.get("http") else {
        return Ok(None);
    };

    let http_obj = http_value.as_object().ok_or_else(|| {
        Status::invalid_argument("request_format.http must be an object when present")
    })?;

    let path = http_obj
        .get("path")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .ok_or_else(|| Status::invalid_argument("request_format.http.path is required"))?;

    if !path.starts_with('/') {
        return Err(Status::invalid_argument(
            "request_format.http.path must start with '/'",
        ));
    }

    let method = http_obj
        .get("method")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .unwrap_or("POST")
        .to_uppercase();

    Ok(Some(HttpMapping {
        method,
        path: path.to_string(),
    }))
}

fn json_value_to_string(value: &JsonValue) -> String {
    match value {
        JsonValue::String(s) => s.clone(),
        JsonValue::Number(n) => n.to_string(),
        JsonValue::Bool(b) => b.to_string(),
        JsonValue::Null => String::new(),
        JsonValue::Array(_) | JsonValue::Object(_) => value.to_string(),
    }
}

fn parse_http_body(body: Bytes) -> Result<(String, HashMap<String, String>), ApiError> {
    if body.is_empty() {
        return Ok((String::new(), HashMap::new()));
    }

    let json: JsonValue = serde_json::from_slice(&body)
        .map_err(|err| ApiError::bad_request(format!("invalid JSON body: {err}")))?;

    match json {
        JsonValue::Object(mut map) => {
            let payload = map
                .remove("payload")
                .map(|value| match value {
                    JsonValue::String(s) => s,
                    JsonValue::Null => String::new(),
                    other => other.to_string(),
                })
                .unwrap_or_default();

            let mut meta: HashMap<String, String> = HashMap::new();

            if let Some(meta_value) = map.remove("meta") {
                if let JsonValue::Object(obj) = meta_value {
                    for (key, val) in obj {
                        meta.insert(key, json_value_to_string(&val));
                    }
                } else {
                    return Err(ApiError::bad_request(
                        "body.meta must be a JSON object".to_string(),
                    ));
                }
            }

            for (key, val) in map {
                meta.insert(key, json_value_to_string(&val));
            }

            Ok((payload, meta))
        }
        JsonValue::String(s) => Ok((s, HashMap::new())),
        JsonValue::Null => Ok((String::new(), HashMap::new())),
        other => Ok((other.to_string(), HashMap::new())),
    }
}

async fn list_apis(State(state): State<HttpState>) -> Json<Vec<ApiRouteEntry>> {
    let routes = state.routes.read().await;
    let mut entries: Vec<ApiRouteEntry> = routes
        .values()
        .map(|route| ApiRouteEntry {
            service: route.service.clone(),
            method: route.method.clone(),
            http_method: route.http_method.clone(),
            path: route.path.clone(),
        })
        .collect();

    entries.sort_by(|a, b| {
        a.service
            .cmp(&b.service)
            .then_with(|| a.path.cmp(&b.path))
            .then_with(|| a.http_method.cmp(&b.http_method))
    });

    Json(entries)
}

async fn handle_http(
    State(state): State<HttpState>,
    Path(path): Path<String>,
    method: Method,
    body: Bytes,
) -> Result<impl IntoResponse, ApiError> {
    let route_path = if path.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", path)
    };

    let route_key = make_route_key(method.as_str(), &route_path);

    let http_route = {
        let routes = state.routes.read().await;
        routes.get(&route_key).cloned().ok_or_else(|| {
            ApiError::not_found(format!(
                "no api route registered for {} {}",
                method, route_path
            ))
        })?
    };

    let (payload, meta) = parse_http_body(body)?;

    let channel = {
        let registry = state.registry.read().await;
        let service = registry.get(&http_route.service).ok_or_else(|| {
            ApiError::not_found(format!(
                "service '{}' is not registered",
                http_route.service
            ))
        })?;

        let registered_method = service.methods.get(&http_route.method).ok_or_else(|| {
            ApiError::not_found(format!(
                "method '{}' is not registered for service '{}'",
                http_route.method, http_route.service
            ))
        })?;

        if registered_method.kind != MethodKind::Unary {
            return Err(ApiError::method_not_allowed(format!(
                "method '{}' is not unary",
                http_route.method
            )));
        }

        service.channel.clone()
    };

    info!(
        http_method = %method,
        path = %route_path,
        service = %http_route.service,
        worker_method = %http_route.method,
        "handling api request",
    );

    let request = ProcessRequest {
        payload,
        meta,
        service: http_route.service.clone(),
        method: http_route.method.clone(),
    };

    let mut worker = WorkerClient::new(channel);
    let response = timeout(state.request_timeout, worker.process(Request::new(request)))
        .await
        .map_err(|_| ApiError::gateway_timeout("worker timeout"))?
        .map_err(ApiError::upstream)?;

    let inner = response.into_inner();
    Ok(Json(ApiResponse {
        result: inner.result,
    }))
}

pub async fn run_http_server(
    state: HttpState,
    addr: SocketAddr,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let router = Router::new()
        .route("/apis", get(list_apis))
        .route("/*path", any(handle_http))
        .with_state(state);

    let listener = TcpListener::bind(addr).await?;
    info!("Engine HTTP listening on {addr}");
    axum::serve(listener, router).await?;
    Ok(())
}
