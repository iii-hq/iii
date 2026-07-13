//! Live span-start push: announce span STARTS to the engine so live trace
//! views (console masthead, per-trace detail) render in-progress worker
//! spans instead of discovering them only when they close.
//!
//! Worker spans normally reach the engine through the batch exporter, which
//! fires `on_end` only — a 3s `harness::turn step` or `provider::*::stream`
//! span is invisible until it finishes. This processor sends ONE extra OTLP
//! frame per span at `on_start`: the same JSON the exporter would emit, with
//! `endTimeUnixNano: "0"` as the live-snapshot sentinel. The engine ingests
//! zero-end spans as `pending` (mirroring its own `LiveSpanProcessor`) and
//! the final span replaces the snapshot in place when the batch exporter
//! delivers it. Engines that predate zero-end ingest (or run OTLP-only)
//! drop the snapshot at ingest — the final-span path is untouched either
//! way, so this is safe to leave on against any engine.
//!
//! Registration order matters: this processor must come AFTER
//! `BaggageSpanProcessor` (`on_start` fires in registration order), so the
//! snapshot already carries the baggage-derived attributes — the console's
//! session attribution (`iii.session.id`, `iii.tag.kind`) relies on them.
//!
//! `send` is a non-blocking `try_send` into the shared telemetry channel; a
//! full channel drops the snapshot with a warning, never blocking the span
//! path. Snapshots deliberately skip the batch processor: batching would
//! trade away the immediacy that is this processor's whole purpose.

use std::sync::Arc;

use opentelemetry::Context;
use opentelemetry::trace::Span as _;
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::error::OTelSdkResult;
use opentelemetry_sdk::trace::{Span, SpanData, SpanProcessor};

use super::connection::SharedEngineConnection;
use super::span_exporter::serialize_span_start_snapshot;
use super::types::PREFIX_TRACES;

pub struct LiveSpanStartProcessor {
    connection: Arc<SharedEngineConnection>,
    resource: Resource,
}

impl LiveSpanStartProcessor {
    pub fn new(connection: Arc<SharedEngineConnection>, resource: Resource) -> Self {
        Self {
            connection,
            resource,
        }
    }
}

impl std::fmt::Debug for LiveSpanStartProcessor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LiveSpanStartProcessor").finish()
    }
}

impl SpanProcessor for LiveSpanStartProcessor {
    fn on_start(&self, span: &mut Span, _cx: &Context) {
        // Sampled-out / non-recording spans never export a final, so a
        // snapshot would be a pending that is never replaced. Skip them.
        if !span.span_context().is_sampled() {
            return;
        }
        let Some(data) = span.exported_data() else {
            return;
        };
        let json = serialize_span_start_snapshot(&data, &self.resource);
        // Best-effort: a dropped snapshot only costs liveness (the final
        // span still lands via the exporter path).
        let _ = self.connection.send(PREFIX_TRACES, json);
    }

    // MUST stay a no-op: the final span lands via the batch exporter;
    // sending it here as well would duplicate every span.
    fn on_end(&self, _span: SpanData) {}

    fn force_flush(&self) -> OTelSdkResult {
        Ok(())
    }

    fn shutdown_with_timeout(&self, _timeout: std::time::Duration) -> OTelSdkResult {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use opentelemetry::baggage::BaggageExt;
    use opentelemetry::trace::{Span as _, Tracer, TracerProvider};
    use opentelemetry::{Context, KeyValue};
    use opentelemetry_sdk::Resource;
    use opentelemetry_sdk::error::OTelSdkResult;
    use opentelemetry_sdk::trace::{
        InMemorySpanExporter, SdkTracerProvider, SimpleSpanProcessor, Span, SpanData, SpanProcessor,
    };

    use super::super::baggage_span_processor::BaggageSpanProcessor;
    use super::super::span_exporter::serialize_span_start_snapshot;

    /// Build one finished SpanData that already went through the baggage
    /// processor, mirroring the provider layout `init_telemetry` sets up.
    fn span_data_with_baggage() -> opentelemetry_sdk::trace::SpanData {
        let exporter = InMemorySpanExporter::default();
        let provider = SdkTracerProvider::builder()
            .with_span_processor(BaggageSpanProcessor)
            .with_span_processor(SimpleSpanProcessor::new(exporter.clone()))
            .build();
        let tracer = provider.tracer("test");
        let cx = Context::new().with_baggage(vec![
            KeyValue::new("iii.session.id", "S-1"),
            KeyValue::new("iii.tag.kind", "harness.turn"),
        ]);
        let span = tracer
            .span_builder("harness::turn step")
            .start_with_context(&tracer, &cx);
        drop(span);
        exporter
            .get_finished_spans()
            .expect("exporter ok")
            .into_iter()
            .next()
            .expect("one span")
    }

    #[test]
    fn start_snapshot_has_zero_end_and_keeps_identity_and_baggage() {
        let data = span_data_with_baggage();
        let resource = Resource::builder_empty()
            .with_attributes([KeyValue::new("service.name", "harness")])
            .build();

        let bytes = serialize_span_start_snapshot(&data, &resource);
        let json: serde_json::Value = serde_json::from_slice(&bytes).expect("valid JSON");

        let span = &json["resourceSpans"][0]["scopeSpans"][0]["spans"][0];
        assert_eq!(
            span["endTimeUnixNano"], "0",
            "zero end is the pending sentinel"
        );
        assert_ne!(span["startTimeUnixNano"], "0");
        assert_eq!(span["name"], "harness::turn step");
        assert_eq!(span["traceId"].as_str().unwrap().len(), 32);
        assert_eq!(span["spanId"].as_str().unwrap().len(), 16);

        // Baggage stamped by the (earlier-registered) BaggageSpanProcessor
        // must already be on the snapshot — session attribution depends on it.
        let attrs = span["attributes"].as_array().expect("attributes array");
        let has = |key: &str, value: &str| {
            attrs
                .iter()
                .any(|a| a["key"] == key && a["value"]["stringValue"] == value)
        };
        assert!(has("iii.session.id", "S-1"), "attrs: {attrs:?}");
        assert!(has("iii.tag.kind", "harness.turn"), "attrs: {attrs:?}");

        // Resource identity rides the envelope like the final-span path.
        let res_attrs = json["resourceSpans"][0]["resource"]["attributes"]
            .as_array()
            .expect("resource attributes");
        assert!(
            res_attrs
                .iter()
                .any(|a| a["key"] == "service.name" && a["value"]["stringValue"] == "harness")
        );
    }

    /// Snapshot-at-start stand-in: same `on_start` body as
    /// `LiveSpanStartProcessor`, capturing the frames instead of sending them
    /// over the connection (which offers no test-side read path).
    #[derive(Debug)]
    struct CaptureStartFrames(Arc<Mutex<Vec<Vec<u8>>>>, Resource);

    impl SpanProcessor for CaptureStartFrames {
        fn on_start(&self, span: &mut Span, _cx: &Context) {
            if !span.span_context().is_sampled() {
                return;
            }
            let Some(data) = span.exported_data() else {
                return;
            };
            self.0
                .lock()
                .unwrap()
                .push(serialize_span_start_snapshot(&data, &self.1));
        }
        fn on_end(&self, _span: SpanData) {}
        fn force_flush(&self) -> OTelSdkResult {
            Ok(())
        }
        fn shutdown_with_timeout(&self, _t: std::time::Duration) -> OTelSdkResult {
            Ok(())
        }
    }

    /// Pins the registration-order contract the live push depends on: the
    /// baggage processor registers FIRST, so a snapshot taken in a later
    /// processor's `on_start` already carries the baggage attributes — this
    /// is what makes a live `harness::turn step` attributable to its session
    /// the moment it starts.
    #[test]
    fn on_start_snapshot_sees_baggage_stamped_by_earlier_processor() {
        let frames = Arc::new(Mutex::new(Vec::new()));
        let resource = Resource::builder_empty().build();
        let provider = SdkTracerProvider::builder()
            .with_span_processor(BaggageSpanProcessor)
            .with_span_processor(CaptureStartFrames(frames.clone(), resource))
            .build();
        let tracer = provider.tracer("test");

        let cx = Context::new().with_baggage(vec![
            KeyValue::new("iii.session.id", "S-live"),
            KeyValue::new("iii.tag.kind", "harness.turn"),
        ]);
        let span = tracer
            .span_builder("harness::turn step")
            .start_with_context(&tracer, &cx);
        // Snapshot is taken at START — end the span only after asserting.
        let captured = frames.lock().unwrap().clone();
        drop(span);

        assert_eq!(captured.len(), 1, "one snapshot per span start");
        let json: serde_json::Value = serde_json::from_slice(&captured[0]).unwrap();
        let snap = &json["resourceSpans"][0]["scopeSpans"][0]["spans"][0];
        assert_eq!(snap["endTimeUnixNano"], "0");
        let attrs = snap["attributes"].as_array().unwrap();
        assert!(
            attrs
                .iter()
                .any(|a| a["key"] == "iii.session.id" && a["value"]["stringValue"] == "S-live"),
            "baggage must be on the start snapshot; attrs: {attrs:?}"
        );
    }
}
