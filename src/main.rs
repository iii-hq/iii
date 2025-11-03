use std::{
    collections::{HashMap, HashSet},
    convert::TryFrom,
    net::SocketAddr,
    pin::Pin,
    sync::Arc,
    time::Duration,
};

use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path, State},
    http::{Method, StatusCode},
    response::{IntoResponse, Response as AxumResponse},
    routing::{any, get},
};
use futures_core::Stream;
use prost_types::{Value, value::Kind as ValueKind};
use serde::Serialize;
use serde_json::{Map as JsonMap, Number as JsonNumber, Value as JsonValue};
use tokio::{net::TcpListener, sync::RwLock, time::timeout};
use tonic::{
    Code, Request, Response, Status,
    transport::{Channel, Server},
};
use tracing::{Level, info, warn};

pub mod engine {
    tonic::include_proto!("engine.v1");
}

use engine::engine_server::{Engine, EngineServer};
use engine::worker_client::WorkerClient;
use engine::{
    ListServicesRequest, ListServicesResponse, MethodDescriptor, MethodKind, ProcessRequest,
    ProcessResponse, RegisterServiceRequest, RegisterServiceResponse, ServiceInfo,
};

#[derive(Clone)]
struct RegisteredMethod {
    kind: MethodKind,
    description: String,
    request_format: Option<Value>,
    response_format: Option<Value>,
}

#[derive(Clone)]
struct RegisteredService {
    service_type: Option<String>,
    methods: HashMap<String, RegisteredMethod>,
    address: String,
    channel: Channel,
    http_routes: Vec<String>,
}

#[derive(Clone)]
struct HttpMapping {
    method: String,
    path: String,
}

#[derive(Clone)]
struct HttpRoute {
    service: String,
    method: String,
    http_method: String,
    path: String,
}

#[derive(Clone)]
struct HttpState {
    registry: Arc<RwLock<HashMap<String, RegisteredService>>>,
    routes: Arc<RwLock<HashMap<String, HttpRoute>>>,
    request_timeout: Duration,
}

#[derive(Clone)]
struct EngineSvc {
    registry: Arc<RwLock<HashMap<String, RegisteredService>>>,
    http_routes: Arc<RwLock<HashMap<String, HttpRoute>>>,
    request_timeout: Duration,
}

type ResponseStream = Pin<Box<dyn Stream<Item = Result<ProcessResponse, Status>> + Send + 'static>>;

fn value_to_summary(value: &Option<Value>) -> String {
    value
        .as_ref()
        .map(value_to_string)
        .unwrap_or_else(|| "unspecified".to_string())
}

fn value_to_string(value: &Value) -> String {
    match value.kind.as_ref() {
        Some(ValueKind::NullValue(_)) => "null".to_string(),
        Some(ValueKind::BoolValue(v)) => v.to_string(),
        Some(ValueKind::NumberValue(v)) => v.to_string(),
        Some(ValueKind::StringValue(v)) => v.clone(),
        Some(ValueKind::ListValue(list)) => {
            let inner = list
                .values
                .iter()
                .map(value_to_string)
                .collect::<Vec<_>>()
                .join(", ");
            format!("[{inner}]")
        }
        Some(ValueKind::StructValue(st)) => {
            let mut parts = st
                .fields
                .iter()
                .map(|(key, val)| format!("{key}: {}", value_to_string(val)))
                .collect::<Vec<_>>();
            parts.sort();
            format!("{{{}}}", parts.join(", "))
        }
        None => "unspecified".to_string(),
    }
}

fn string_value<S: Into<String>>(value: S) -> Value {
    Value {
        kind: Some(ValueKind::StringValue(value.into())),
    }
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

fn parse_http_mapping(request_format: &Option<Value>) -> Result<Option<HttpMapping>, Status> {
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

fn make_route_key(method: &str, path: &str) -> String {
    format!("{} {}", method.to_uppercase(), path)
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

async fn run_http_server(
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

#[tonic::async_trait]
impl Engine for EngineSvc {
    async fn process(
        &self,
        req: Request<ProcessRequest>,
    ) -> Result<Response<ProcessResponse>, Status> {
        let metadata = req.metadata().clone();
        let inner = req.into_inner();

        if inner.service.is_empty() {
            return Err(Status::invalid_argument("service field is required"));
        }

        if inner.method.is_empty() {
            return Err(Status::invalid_argument("method field is required"));
        }

        let service_name = inner.service.clone();
        let method_name = inner.method.clone();

        let (channel, target_addr, target_kind, method_kind) = {
            let registry = self.registry.read().await;
            let service = registry.get(&service_name).ok_or_else(|| {
                Status::not_found(format!("service '{service_name}' is not registered"))
            })?;

            let method = service.methods.get(&method_name).ok_or_else(|| {
                Status::not_found(format!(
                    "method '{method_name}' is not registered for service '{service_name}'"
                ))
            })?;
            let method_kind = method.kind;

            if method_kind == MethodKind::ServerStreaming {
                return Err(Status::failed_precondition(format!(
                    "method '{method_name}' is server-streaming; invoke via StreamProcess"
                )));
            }

            (
                service.channel.clone(),
                service.address.clone(),
                service
                    .service_type
                    .clone()
                    .unwrap_or_else(|| "unspecified".to_string()),
                method_kind,
            )
        };

        info!(
            service = %service_name,
            method = %method_name,
            target = %target_addr,
            service_type = %target_kind,
            method_kind = %method_kind.as_str_name(),
            "forwarding request to worker",
        );

        let mut out = Request::new(inner);
        *out.metadata_mut() = metadata;

        let mut worker = WorkerClient::new(channel);
        let fut = worker.process(out);
        let resp = timeout(self.request_timeout, fut)
            .await
            .map_err(|_| Status::deadline_exceeded("worker timeout"))?
            .map_err(|e| Status::unavailable(format!("worker error: {e}")))?;

        Ok(resp)
    }

    type StreamProcessStream = ResponseStream;

    async fn stream_process(
        &self,
        req: Request<ProcessRequest>,
    ) -> Result<Response<Self::StreamProcessStream>, Status> {
        let metadata = req.metadata().clone();
        let inner = req.into_inner();

        if inner.service.is_empty() {
            return Err(Status::invalid_argument("service field is required"));
        }

        if inner.method.is_empty() {
            return Err(Status::invalid_argument("method field is required"));
        }

        let service_name = inner.service.clone();
        let method_name = inner.method.clone();

        let (channel, target_addr, target_kind) = {
            let registry = self.registry.read().await;
            let service = registry.get(&service_name).ok_or_else(|| {
                Status::not_found(format!("service '{service_name}' is not registered"))
            })?;

            let method = service.methods.get(&method_name).ok_or_else(|| {
                Status::not_found(format!(
                    "method '{method_name}' is not registered for service '{service_name}'"
                ))
            })?;
            let method_kind = method.kind;

            if method_kind != MethodKind::ServerStreaming {
                return Err(Status::failed_precondition(format!(
                    "method '{method_name}' is not server-streaming; invoke via Process"
                )));
            }

            (
                service.channel.clone(),
                service.address.clone(),
                service
                    .service_type
                    .clone()
                    .unwrap_or_else(|| "unspecified".to_string()),
            )
        };

        info!(
            service = %service_name,
            method = %method_name,
            target = %target_addr,
            service_type = %target_kind,
            "forwarding streaming request to worker",
        );

        let mut out = Request::new(inner);
        *out.metadata_mut() = metadata;

        let mut worker = WorkerClient::new(channel);
        let fut = worker.stream_process(out);
        let response = timeout(self.request_timeout, fut)
            .await
            .map_err(|_| Status::deadline_exceeded("worker timeout"))?
            .map_err(|e| Status::unavailable(format!("worker error: {e}")))?;

        let stream = response.into_inner();
        let output: ResponseStream = Box::pin(stream);
        Ok(Response::new(output))
    }

    async fn list_services(
        &self,
        _request: Request<ListServicesRequest>,
    ) -> Result<Response<ListServicesResponse>, Status> {
        let registry = self.registry.read().await;
        let services = registry
            .iter()
            .map(|(name, svc)| {
                let methods = svc
                    .methods
                    .iter()
                    .map(|(method_name, registered)| MethodDescriptor {
                        name: method_name.clone(),
                        description: registered.description.clone(),
                        kind: registered.kind as i32,
                        request_format: registered.request_format.clone(),
                        response_format: registered.response_format.clone(),
                    })
                    .collect::<Vec<MethodDescriptor>>();

                ServiceInfo {
                    name: name.clone(),
                    address: svc.address.clone(),
                    service_type: svc.service_type.clone().unwrap_or_default(),
                    methods,
                }
            })
            .collect::<Vec<ServiceInfo>>();

        Ok(Response::new(ListServicesResponse { services }))
    }

    async fn register_service(
        &self,
        request: Request<RegisterServiceRequest>,
    ) -> Result<Response<RegisterServiceResponse>, Status> {
        let RegisterServiceRequest {
            name,
            address,
            service_type: raw_service_type,
            methods,
        } = request.into_inner();

        if name.trim().is_empty() {
            return Err(Status::invalid_argument("service name is required"));
        }

        if address.trim().is_empty() {
            return Err(Status::invalid_argument("worker address is required"));
        }

        let service_type = if raw_service_type.trim().is_empty() {
            None
        } else {
            Some(raw_service_type.trim().to_string())
        };
        let service_type_label = service_type
            .clone()
            .unwrap_or_else(|| "unspecified".to_string());
        let is_api_service = service_type
            .as_ref()
            .map(|ty| ty.eq_ignore_ascii_case("api"))
            .unwrap_or(false);

        let mut method_map: HashMap<String, RegisteredMethod> = HashMap::new();
        let mut method_summaries: Vec<String> = Vec::new();
        let mut http_route_entries: Vec<(String, HttpRoute)> = Vec::new();

        for m in methods.into_iter() {
            let method_name = m.name.trim();
            if method_name.is_empty() {
                continue;
            }

            let kind = MethodKind::try_from(m.kind).unwrap_or(MethodKind::Unary);
            let description = m.description.trim().to_owned();
            let request_format = m.request_format;
            let response_format = m.response_format;
            let request_summary = value_to_summary(&request_format);
            let response_summary = value_to_summary(&response_format);
            let http_mapping = parse_http_mapping(&request_format)?;

            if is_api_service && http_mapping.is_none() {
                return Err(Status::invalid_argument(format!(
                    "api service '{name}' method '{method_name}' must specify http metadata in request_format.http"
                )));
            }

            if let Some(ref http) = http_mapping {
                let route_key = make_route_key(&http.method, &http.path);
                info!(
                    service = %name,
                    method = %method_name,
                    http_method = %http.method,
                    http_path = %http.path,
                    "registered http route for api service",
                );
                http_route_entries.push((
                    route_key,
                    HttpRoute {
                        service: name.clone(),
                        method: method_name.to_owned(),
                        http_method: http.method.clone(),
                        path: http.path.clone(),
                    },
                ));
            }

            method_summaries.push(format!(
                "{method_name}: request={request_summary} response={response_summary}"
            ));

            method_map.insert(
                method_name.to_owned(),
                RegisteredMethod {
                    kind,
                    description,
                    request_format,
                    response_format,
                },
            );
        }

        if method_map.is_empty() {
            return Err(Status::invalid_argument(
                "at least one named method must be provided",
            ));
        }

        if is_api_service && http_route_entries.is_empty() {
            return Err(Status::invalid_argument(format!(
                "api service '{name}' registered no http routes"
            )));
        }

        let mut seen_routes: HashSet<String> = HashSet::new();
        for (key, _) in &http_route_entries {
            if !seen_routes.insert(key.clone()) {
                return Err(Status::invalid_argument(format!(
                    "duplicate http route '{key}' within service '{name}'"
                )));
            }
        }

        let existing_http_keys: HashSet<String> = {
            let registry = self.registry.read().await;
            registry
                .get(&name)
                .map(|svc| svc.http_routes.clone())
                .unwrap_or_default()
                .into_iter()
                .collect()
        };

        {
            let routes = self.http_routes.read().await;
            for (key, _) in &http_route_entries {
                if let Some(existing) = routes.get(key) {
                    if existing.service != name && !existing_http_keys.contains(key) {
                        return Err(Status::already_exists(format!(
                            "http route '{key}' already registered by service '{}'",
                            existing.service
                        )));
                    }
                }
            }
        }

        let method_list = if method_summaries.is_empty() {
            String::new()
        } else {
            method_summaries.join(" | ")
        };

        let endpoint = Channel::from_shared(address.clone()).map_err(|err| {
            Status::invalid_argument(format!("invalid worker address '{address}': {err}"))
        })?;

        // Attempt the connection so we fail fast when a worker is unreachable.
        let channel = endpoint
            .connect()
            .await
            .map_err(|err| Status::unavailable(format!("unable to reach worker: {err}")))?;

        let http_route_keys: Vec<String> = http_route_entries
            .iter()
            .map(|(key, _)| key.clone())
            .collect();

        let registered = RegisteredService {
            service_type: service_type.clone(),
            methods: method_map,
            address: address.clone(),
            channel,
            http_routes: http_route_keys,
        };

        let (replaced, old_service, notification_targets) = {
            let mut registry = self.registry.write().await;
            let old = registry.insert(name.clone(), registered);
            let targets = registry
                .iter()
                .filter(|(svc_name, _)| svc_name.as_str() != name)
                .map(|(svc_name, svc)| {
                    (
                        svc_name.clone(),
                        svc.address.clone(),
                        svc.service_type.clone(),
                        svc.methods.contains_key("service_registered"),
                        svc.channel.clone(),
                    )
                })
                .collect::<Vec<_>>();
            (old.is_some(), old, targets)
        };

        {
            let mut routes = self.http_routes.write().await;
            if let Some(old) = old_service {
                for key in old.http_routes {
                    routes.remove(&key);
                }
            }
            for (key, route) in &http_route_entries {
                routes.insert(key.clone(), route.clone());
            }
        }

        if replaced {
            warn!(
                service = %name,
                address = %address,
                service_type = %service_type_label,
                "updated service registration",
            );
        } else {
            info!(
                service = %name,
                address = %address,
                service_type = %service_type_label,
                "registered service",
            );
        }

        let registration_kind = if replaced { "updated" } else { "new" };

        for (other_name, other_address, other_service_type, accepts_notification, channel) in
            notification_targets
        {
            if !accepts_notification {
                info!(
                    service = %other_name,
                    "skipping service registration notification; target did not register 'service_registered'",
                );
                continue;
            }

            let target_kind_label = other_service_type.unwrap_or_else(|| "unspecified".to_string());

            let mut meta = HashMap::new();
            meta.insert("event".to_string(), "service_registered".to_string());
            meta.insert(
                "registration_kind".to_string(),
                registration_kind.to_string(),
            );
            meta.insert("new_service_name".to_string(), name.clone());
            meta.insert("new_service_address".to_string(), address.clone());
            if let Some(ref ty) = service_type {
                meta.insert("new_service_type".to_string(), ty.clone());
            }
            if !method_list.is_empty() {
                meta.insert("new_service_methods".to_string(), method_list.clone());
            }

            let notification = ProcessRequest {
                payload: format!("service '{name}' is now available at {address}"),
                meta,
                service: other_name.clone(),
                method: "service_registered".to_string(),
            };

            let mut worker = WorkerClient::new(channel);
            let fut = worker.process(Request::new(notification));
            match timeout(self.request_timeout, fut).await {
                Ok(Ok(_)) => info!(
                    service = %other_name,
                    target_service_type = %target_kind_label,
                    target_address = %other_address,
                    new_service = %name,
                    new_service_address = %address,
                    new_service_type = %service_type_label,
                    "notified service about new registration",
                ),
                Ok(Err(err)) => warn!(
                    service = %other_name,
                    target_service_type = %target_kind_label,
                    target_address = %other_address,
                    new_service = %name,
                    new_service_address = %address,
                    new_service_type = %service_type_label,
                    error = %err,
                    "failed to notify service about registration",
                ),
                Err(_) => warn!(
                    service = %other_name,
                    target_service_type = %target_kind_label,
                    target_address = %other_address,
                    new_service = %name,
                    new_service_address = %address,
                    new_service_type = %service_type_label,
                    "notification to service timed out",
                ),
            }
        }

        Ok(Response::new(RegisterServiceResponse {
            accepted: true,
            message: format!("service '{name}' registered"),
        }))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_max_level(Level::INFO)
        .init();

    let engine_addr_str = std::env::var("ENGINE_ADDR").unwrap_or_else(|_| "0.0.0.0:50051".into());
    let http_addr_str = std::env::var("HTTP_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".into());

    let grpc_addr: SocketAddr = engine_addr_str.parse()?;
    let http_addr: SocketAddr = http_addr_str.parse()?;

    let worker_addr =
        std::env::var("WORKER_ADDR").unwrap_or_else(|_| "http://127.0.0.1:50052".into());

    let svc = EngineSvc {
        registry: Arc::new(RwLock::new(HashMap::new())),
        http_routes: Arc::new(RwLock::new(HashMap::new())),
        request_timeout: Duration::from_secs(3),
    };

    // Optionally register a default worker for backwards compatibility.
    if let Ok(endpoint) = Channel::from_shared(worker_addr.clone()) {
        let channel = endpoint.connect_lazy();
        let mut methods = HashMap::new();
        methods.insert(
            "process".to_string(),
            RegisteredMethod {
                kind: MethodKind::Unary,
                description: "Default unary process handler".to_string(),
                request_format: Some(string_value("ProcessRequest")),
                response_format: Some(string_value("ProcessResponse")),
            },
        );
        let default_service = RegisteredService {
            service_type: Some("default".to_string()),
            methods,
            address: worker_addr.clone(),
            channel,
            http_routes: Vec::new(),
        };
        svc.registry
            .write()
            .await
            .insert("default".to_string(), default_service);
        info!(address = %worker_addr, "registered default worker under service 'default'");
    } else {
        warn!(address = %worker_addr, "invalid default worker address; skipping pre-registration");
    }

    let http_state = HttpState {
        registry: Arc::clone(&svc.registry),
        routes: Arc::clone(&svc.http_routes),
        request_timeout: svc.request_timeout,
    };

    let grpc_future = async {
        info!(address = %grpc_addr, "Engine gRPC listening");
        Server::builder()
            .add_service(EngineServer::new(svc))
            .serve(grpc_addr)
            .await
            .map_err(|err| Box::new(err) as Box<dyn std::error::Error + Send + Sync>)
    };

    tokio::try_join!(grpc_future, run_http_server(http_state, http_addr))?;

    Ok(())
}
