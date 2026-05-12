//! Shared mock OTLP gRPC collector for the `otel_forwarder_*` integration
//! tests. Extracted per #1618 review (Bridgebuilder F7) so the three
//! forwarder regression tests share one collector implementation and
//! one readiness-probe contract — keeps the suite in lockstep when the
//! `SDK_SPAN_FORWARDER` shape changes.
//!
//! Two flavors exist because the headers test needs to capture
//! `tonic::MetadataMap` (not the request body) while the
//! resource-preservation / partial-batch tests need the body:
//!
//! - [`spawn_body_capturing_collector`] — records the inbound
//!   `ExportTraceServiceRequest` payloads.
//! - [`spawn_metadata_capturing_collector`] — records the inbound
//!   `tonic::MetadataMap` per request.
//!
//! Both share the same readiness-probe behaviour (bounded
//! `TcpStream::connect` retry; replaces the older 50ms-sleep
//! heuristic that flaked on loaded CI runners).

use std::sync::Arc;
use std::time::Duration;

use opentelemetry_proto::tonic::collector::trace::v1::{
    ExportTraceServiceRequest, ExportTraceServiceResponse,
    trace_service_server::{TraceService, TraceServiceServer},
};
use tokio::sync::Mutex;
use tonic::metadata::MetadataMap;
use tonic::transport::Server;
use tonic::{Request, Response, Status};

// ─── Body-capturing variant ────────────────────────────────────────────────

#[derive(Default, Clone)]
struct CapturingBody {
    received: Arc<Mutex<Vec<ExportTraceServiceRequest>>>,
}

#[tonic::async_trait]
impl TraceService for CapturingBody {
    async fn export(
        &self,
        request: Request<ExportTraceServiceRequest>,
    ) -> Result<Response<ExportTraceServiceResponse>, Status> {
        self.received.lock().await.push(request.into_inner());
        Ok(Response::new(ExportTraceServiceResponse::default()))
    }
}

pub async fn spawn_body_capturing_collector() -> (String, Arc<Mutex<Vec<ExportTraceServiceRequest>>>)
{
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let endpoint = format!("http://{addr}");

    let service = CapturingBody::default();
    let received = service.received.clone();

    tokio::spawn(async move {
        let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);
        Server::builder()
            .add_service(TraceServiceServer::new(service))
            .serve_with_incoming(incoming)
            .await
            .expect("mock OTLP collector failed to start; see tonic transport error above")
    });

    wait_for_collector_ready(addr).await;

    (endpoint, received)
}

// ─── Metadata-capturing variant ───────────────────────────────────────────

#[derive(Default, Clone)]
struct CapturingMetadata {
    received: Arc<Mutex<Vec<MetadataMap>>>,
}

#[tonic::async_trait]
impl TraceService for CapturingMetadata {
    async fn export(
        &self,
        request: Request<ExportTraceServiceRequest>,
    ) -> Result<Response<ExportTraceServiceResponse>, Status> {
        self.received.lock().await.push(request.metadata().clone());
        Ok(Response::new(ExportTraceServiceResponse::default()))
    }
}

pub async fn spawn_metadata_capturing_collector() -> (String, Arc<Mutex<Vec<MetadataMap>>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let endpoint = format!("http://{addr}");

    let service = CapturingMetadata::default();
    let received = service.received.clone();

    tokio::spawn(async move {
        let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);
        Server::builder()
            .add_service(TraceServiceServer::new(service))
            .serve_with_incoming(incoming)
            .await
            .expect("mock OTLP collector failed to start; see tonic transport error above")
    });

    wait_for_collector_ready(addr).await;

    (endpoint, received)
}

// ─── Readiness probe (shared) ─────────────────────────────────────────────

/// Active TCP-connect probe replaces the older 50ms-sleep heuristic.
/// `TcpListener::bind` already guarantees the kernel-level listening
/// socket is open before we get here; the remaining race is purely "is
/// tonic's accept loop being polled?". A successful connect-then-drop
/// is sufficient evidence the accept loop is live.
async fn wait_for_collector_ready(addr: std::net::SocketAddr) {
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        match tokio::net::TcpStream::connect(addr).await {
            Ok(_) => return,
            Err(_) if std::time::Instant::now() < deadline => {
                tokio::task::yield_now().await;
            }
            Err(e) => panic!("mock collector not reachable within 2s: {e}"),
        }
    }
}
