// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use std::env;

use opentelemetry_otlp::{
    ExporterBuildError, Protocol, WithExportConfig, WithTonicConfig,
    tonic_types::transport::ClientTlsConfig,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OtlpSignal {
    Traces,
    Metrics,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OtlpProtocol {
    Grpc,
    HttpProtobuf,
}

pub(crate) fn protocol_from_env(signal: OtlpSignal) -> OtlpProtocol {
    let signal_var = match signal {
        OtlpSignal::Traces => "OTEL_EXPORTER_OTLP_TRACES_PROTOCOL",
        OtlpSignal::Metrics => "OTEL_EXPORTER_OTLP_METRICS_PROTOCOL",
    };

    env::var(signal_var)
        .or_else(|_| env::var("OTEL_EXPORTER_OTLP_PROTOCOL"))
        .ok()
        .as_deref()
        .map(protocol_from_value)
        .unwrap_or(OtlpProtocol::Grpc)
}

fn protocol_from_value(value: &str) -> OtlpProtocol {
    match value.trim().to_ascii_lowercase().as_str() {
        "http/protobuf" => OtlpProtocol::HttpProtobuf,
        "grpc" => OtlpProtocol::Grpc,
        other => {
            tracing::warn!(
                protocol = other,
                "Unsupported OTLP protocol; falling back to grpc"
            );
            OtlpProtocol::Grpc
        }
    }
}

fn signal_http_path(signal: OtlpSignal) -> &'static str {
    match signal {
        OtlpSignal::Traces => "/v1/traces",
        OtlpSignal::Metrics => "/v1/metrics",
    }
}

fn strip_otlp_signal_path(endpoint: &str) -> &str {
    ["/v1/traces", "/v1/metrics", "/v1/logs"]
        .iter()
        .find_map(|signal_path| endpoint.strip_suffix(signal_path))
        .unwrap_or(endpoint)
}

pub(crate) fn http_endpoint_for_signal(endpoint: &str, signal: OtlpSignal) -> String {
    let trimmed = endpoint.trim_end_matches('/');
    let base = strip_otlp_signal_path(trimmed);

    format!("{base}{}", signal_http_path(signal))
}

fn endpoint_uses_https(endpoint: &str) -> bool {
    endpoint
        .trim_start()
        .get(..8)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("https://"))
}

pub(crate) fn build_span_exporter(
    endpoint: &str,
) -> Result<opentelemetry_otlp::SpanExporter, ExporterBuildError> {
    match protocol_from_env(OtlpSignal::Traces) {
        OtlpProtocol::Grpc => {
            let builder = opentelemetry_otlp::SpanExporter::builder()
                .with_tonic()
                .with_endpoint(endpoint);

            if endpoint_uses_https(endpoint) {
                builder
                    .with_tls_config(ClientTlsConfig::new().with_enabled_roots())
                    .build()
            } else {
                builder.build()
            }
        }
        OtlpProtocol::HttpProtobuf => {
            let endpoint = http_endpoint_for_signal(endpoint, OtlpSignal::Traces);
            opentelemetry_otlp::SpanExporter::builder()
                .with_http()
                .with_protocol(Protocol::HttpBinary)
                .with_endpoint(endpoint)
                .build()
        }
    }
}

pub(crate) fn build_metric_exporter(
    endpoint: &str,
) -> Result<opentelemetry_otlp::MetricExporter, ExporterBuildError> {
    match protocol_from_env(OtlpSignal::Metrics) {
        OtlpProtocol::Grpc => {
            let builder = opentelemetry_otlp::MetricExporter::builder()
                .with_tonic()
                .with_endpoint(endpoint);

            if endpoint_uses_https(endpoint) {
                builder
                    .with_tls_config(ClientTlsConfig::new().with_enabled_roots())
                    .build()
            } else {
                builder.build()
            }
        }
        OtlpProtocol::HttpProtobuf => {
            let endpoint = http_endpoint_for_signal(endpoint, OtlpSignal::Metrics);
            opentelemetry_otlp::MetricExporter::builder()
                .with_http()
                .with_protocol(Protocol::HttpBinary)
                .with_endpoint(endpoint)
                .build()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{future::Future, time::Duration};

    use opentelemetry::trace::{Span as _, Tracer as _, TracerProvider as _};
    use opentelemetry_sdk::{
        metrics::{data::ResourceMetrics, exporter::PushMetricExporter},
        trace::SdkTracerProvider,
    };
    use serial_test::serial;
    use tokio::{io::AsyncReadExt, net::TcpListener};

    async fn capture_first_write<F, Fut>(scheme: &str, run_export: F) -> anyhow::Result<Vec<u8>>
    where
        F: FnOnce(String) -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let endpoint = format!("{scheme}://{addr}");

        let mut accept = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await?;
            let mut buf = [0_u8; 2048];
            let n = tokio::time::timeout(Duration::from_secs(3), socket.read(&mut buf)).await??;
            Ok::<_, anyhow::Error>(buf[..n].to_vec())
        });

        let mut export = tokio::spawn(run_export(endpoint));
        let bytes = tokio::time::timeout(Duration::from_secs(5), &mut accept).await???;
        if tokio::time::timeout(Duration::from_secs(2), &mut export)
            .await
            .is_err()
        {
            export.abort();
            let _ = export.await;
        }

        Ok(bytes)
    }

    async fn export_one_span(exporter: opentelemetry_otlp::SpanExporter) {
        let provider = SdkTracerProvider::builder()
            .with_batch_exporter(exporter)
            .build();
        let tracer = provider.tracer("otel-export-transport-test");
        let mut span = tracer.start("trace-export-transport");
        span.end();

        let _ = provider.shutdown_with_timeout(Duration::from_secs(1));
    }

    fn export_one_metric(exporter: opentelemetry_otlp::MetricExporter) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build metric export runtime");
        let metrics = ResourceMetrics::default();
        let _ = runtime.block_on(exporter.export(&metrics));
        drop(exporter);
    }

    fn build_span_exporter_for_test(
        endpoint: &str,
        protocol: Option<&str>,
    ) -> opentelemetry_otlp::SpanExporter {
        temp_env::with_vars(
            [
                ("OTEL_EXPORTER_OTLP_PROTOCOL", protocol),
                ("OTEL_EXPORTER_OTLP_TRACES_PROTOCOL", None),
                ("OTEL_EXPORTER_OTLP_HEADERS", None),
                ("OTEL_EXPORTER_OTLP_TIMEOUT", Some("250")),
                ("OTEL_EXPORTER_OTLP_TRACES_TIMEOUT", Some("250")),
            ],
            || build_span_exporter(endpoint).expect("build span exporter"),
        )
    }

    fn build_metric_exporter_for_test(
        endpoint: &str,
        protocol: Option<&str>,
    ) -> opentelemetry_otlp::MetricExporter {
        temp_env::with_vars(
            [
                ("OTEL_EXPORTER_OTLP_PROTOCOL", protocol),
                ("OTEL_EXPORTER_OTLP_METRICS_PROTOCOL", None),
                ("OTEL_EXPORTER_OTLP_HEADERS", None),
                ("OTEL_EXPORTER_OTLP_TIMEOUT", Some("250")),
                ("OTEL_EXPORTER_OTLP_METRICS_TIMEOUT", Some("250")),
            ],
            || build_metric_exporter(endpoint).expect("build metric exporter"),
        )
    }

    fn assert_http2_preface(bytes: &[u8]) {
        assert!(
            bytes.starts_with(b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n"),
            "expected gRPC HTTP/2 preface, got bytes: {bytes:?}"
        );
    }

    fn assert_tls_client_hello(bytes: &[u8]) {
        assert!(
            bytes.len() >= 3 && bytes[0] == 0x16 && bytes[1] == 0x03,
            "expected TLS ClientHello, got bytes: {bytes:?}"
        );
    }

    #[test]
    #[serial]
    fn protocol_defaults_to_grpc() {
        temp_env::with_vars(
            [
                ("OTEL_EXPORTER_OTLP_PROTOCOL", None::<&str>),
                ("OTEL_EXPORTER_OTLP_TRACES_PROTOCOL", None::<&str>),
                ("OTEL_EXPORTER_OTLP_METRICS_PROTOCOL", None::<&str>),
            ],
            || {
                assert_eq!(protocol_from_env(OtlpSignal::Traces), OtlpProtocol::Grpc);
                assert_eq!(protocol_from_env(OtlpSignal::Metrics), OtlpProtocol::Grpc);
            },
        );
    }

    #[test]
    #[serial]
    fn protocol_uses_global_http_protobuf() {
        temp_env::with_vars(
            [
                ("OTEL_EXPORTER_OTLP_PROTOCOL", Some("http/protobuf")),
                ("OTEL_EXPORTER_OTLP_TRACES_PROTOCOL", None),
                ("OTEL_EXPORTER_OTLP_METRICS_PROTOCOL", None),
            ],
            || {
                assert_eq!(
                    protocol_from_env(OtlpSignal::Traces),
                    OtlpProtocol::HttpProtobuf
                );
                assert_eq!(
                    protocol_from_env(OtlpSignal::Metrics),
                    OtlpProtocol::HttpProtobuf
                );
            },
        );
    }

    #[test]
    #[serial]
    fn signal_protocol_overrides_global_protocol() {
        temp_env::with_vars(
            [
                ("OTEL_EXPORTER_OTLP_PROTOCOL", Some("grpc")),
                ("OTEL_EXPORTER_OTLP_TRACES_PROTOCOL", Some("http/protobuf")),
                ("OTEL_EXPORTER_OTLP_METRICS_PROTOCOL", None),
            ],
            || {
                assert_eq!(
                    protocol_from_env(OtlpSignal::Traces),
                    OtlpProtocol::HttpProtobuf
                );
                assert_eq!(protocol_from_env(OtlpSignal::Metrics), OtlpProtocol::Grpc);
            },
        );
    }

    #[test]
    fn unknown_protocol_falls_back_to_grpc() {
        assert_eq!(protocol_from_value("http/json"), OtlpProtocol::Grpc);
        assert_eq!(protocol_from_value("bad-value"), OtlpProtocol::Grpc);
    }

    #[test]
    fn http_endpoint_appends_trace_path_to_base_endpoint() {
        assert_eq!(
            http_endpoint_for_signal("https://collector.example.com", OtlpSignal::Traces),
            "https://collector.example.com/v1/traces"
        );
    }

    #[test]
    fn http_endpoint_appends_metric_path_to_base_endpoint() {
        assert_eq!(
            http_endpoint_for_signal("https://collector.example.com/", OtlpSignal::Metrics),
            "https://collector.example.com/v1/metrics"
        );
    }

    #[test]
    fn http_endpoint_does_not_double_append_signal_path() {
        assert_eq!(
            http_endpoint_for_signal(
                "https://collector.example.com/v1/traces",
                OtlpSignal::Traces
            ),
            "https://collector.example.com/v1/traces"
        );
        assert_eq!(
            http_endpoint_for_signal(
                "https://collector.example.com/v1/metrics",
                OtlpSignal::Metrics
            ),
            "https://collector.example.com/v1/metrics"
        );
    }

    #[test]
    fn http_endpoint_replaces_existing_other_signal_path() {
        assert_eq!(
            http_endpoint_for_signal(
                "https://collector.example.com/v1/traces",
                OtlpSignal::Metrics
            ),
            "https://collector.example.com/v1/metrics"
        );
        assert_eq!(
            http_endpoint_for_signal(
                "https://collector.example.com/v1/metrics",
                OtlpSignal::Traces
            ),
            "https://collector.example.com/v1/traces"
        );
        assert_eq!(
            http_endpoint_for_signal("https://collector.example.com/v1/logs", OtlpSignal::Traces),
            "https://collector.example.com/v1/traces"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[serial]
    async fn trace_exporter_builder_defaults_to_cleartext_grpc() -> anyhow::Result<()> {
        let bytes = capture_first_write("http", |endpoint| async move {
            let exporter = build_span_exporter_for_test(&endpoint, None);
            export_one_span(exporter).await;
        })
        .await?;

        assert_http2_preface(&bytes);
        Ok(())
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[serial]
    async fn trace_exporter_builder_uses_tls_for_https_grpc_endpoint() -> anyhow::Result<()> {
        let bytes = capture_first_write("https", |endpoint| async move {
            let exporter = build_span_exporter_for_test(&endpoint, None);
            export_one_span(exporter).await;
        })
        .await?;

        assert_tls_client_hello(&bytes);
        Ok(())
    }

    #[serial]
    #[test]
    fn trace_exporter_builder_honors_http_protobuf_protocol_env() {
        let exporter = build_span_exporter_for_test("http://127.0.0.1:4318", Some("http/protobuf"));
        drop(exporter);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[serial]
    async fn metric_exporter_builder_defaults_to_cleartext_grpc() -> anyhow::Result<()> {
        let bytes = capture_first_write("http", |endpoint| async move {
            let exporter = build_metric_exporter_for_test(&endpoint, None);
            let _ = tokio::task::spawn_blocking(move || export_one_metric(exporter)).await;
        })
        .await?;

        assert_http2_preface(&bytes);
        Ok(())
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[serial]
    async fn metric_exporter_builder_uses_tls_for_https_grpc_endpoint() -> anyhow::Result<()> {
        let bytes = capture_first_write("https", |endpoint| async move {
            let exporter = build_metric_exporter_for_test(&endpoint, None);
            let _ = tokio::task::spawn_blocking(move || export_one_metric(exporter)).await;
        })
        .await?;

        assert_tls_client_hello(&bytes);
        Ok(())
    }

    #[serial]
    #[test]
    fn metric_exporter_builder_honors_http_protobuf_protocol_env() {
        let exporter =
            build_metric_exporter_for_test("http://127.0.0.1:4318", Some("http/protobuf"));
        drop(exporter);
    }
}
