use std::{collections::HashMap, convert::TryFrom, pin::Pin, sync::Arc, time::Duration};

use futures_core::Stream;
use tokio::{sync::RwLock, time::timeout};
use tonic::{
    Request, Response, Status,
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
    request_format: String,
    response_format: String,
}

#[derive(Clone)]
struct RegisteredService {
    service_type: Option<String>,
    methods: HashMap<String, RegisteredMethod>,
    address: String,
    channel: Channel,
}

#[derive(Clone)]
struct EngineSvc {
    registry: Arc<RwLock<HashMap<String, RegisteredService>>>,
    request_timeout: Duration,
}

type ResponseStream = Pin<Box<dyn Stream<Item = Result<ProcessResponse, Status>> + Send + 'static>>;

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

        let mut method_map: HashMap<String, RegisteredMethod> = HashMap::new();
        let mut method_summaries: Vec<String> = Vec::new();

        for m in methods.into_iter() {
            let method_name = m.name.trim();
            if method_name.is_empty() {
                continue;
            }

            let kind = MethodKind::try_from(m.kind).unwrap_or(MethodKind::Unary);
            let description = m.description.trim().to_owned();
            let request_format = m.request_format.trim().to_owned();
            let response_format = m.response_format.trim().to_owned();
            let request_summary = if request_format.is_empty() {
                "unspecified"
            } else {
                request_format.as_str()
            };
            let response_summary = if response_format.is_empty() {
                "unspecified"
            } else {
                response_format.as_str()
            };

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

        let service_type = if raw_service_type.trim().is_empty() {
            None
        } else {
            Some(raw_service_type)
        };
        let service_type_label = service_type
            .clone()
            .unwrap_or_else(|| "unspecified".to_string());

        let registered = RegisteredService {
            service_type: service_type.clone(),
            methods: method_map,
            address: address.clone(),
            channel,
        };

        let (replaced, notification_targets) = {
            let mut registry = self.registry.write().await;
            let replaced = registry.insert(name.clone(), registered).is_some();
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
            (replaced, targets)
        };

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
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_max_level(Level::INFO)
        .init();

    let engine_addr = std::env::var("ENGINE_ADDR").unwrap_or_else(|_| "0.0.0.0:50051".into());

    let worker_addr =
        std::env::var("WORKER_ADDR").unwrap_or_else(|_| "http://127.0.0.1:50052".into());

    let svc = EngineSvc {
        registry: Arc::new(RwLock::new(HashMap::new())),
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
                request_format: "ProcessRequest".to_string(),
                response_format: "ProcessResponse".to_string(),
            },
        );
        let default_service = RegisteredService {
            service_type: Some("default".to_string()),
            methods,
            address: worker_addr.clone(),
            channel,
        };
        svc.registry
            .write()
            .await
            .insert("default".to_string(), default_service);
        info!(address = %worker_addr, "registered default worker under service 'default'");
    } else {
        warn!(address = %worker_addr, "invalid default worker address; skipping pre-registration");
    }

    info!("Engine gRPC listening on {engine_addr}");
    Server::builder()
        .add_service(EngineServer::new(svc))
        .serve(engine_addr.parse()?)
        .await?;

    Ok(())
}
