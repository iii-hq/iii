//! Baggage -> span attribute processor.
//!
//! Copies EVERY baggage entry onto every span started in its scope,
//! unconditionally — the same contract as the upstream OTel contrib
//! `BaggageSpanProcessor`. There is deliberately no key filtering here:
//! a filtering policy baked into worker binaries has to be kept in
//! lockstep across every SDK language and every deployed worker, and a
//! stale binary silently drops newer keys. Which attributes *mean*
//! something (e.g. the `iii.tag.*` trace-tag namespace, `iii.session.*`)
//! is a query-side convention owned by the engine's traces API, where it
//! can evolve without touching workers.
//!
//! Baggage in the iii ecosystem is set by iii code (turn identity, trace
//! tags, function routing), so span attributes stay bounded by what
//! callers deliberately stamp; W3C propagator limits bound what can
//! arrive from outside.

use opentelemetry::baggage::BaggageExt;
use opentelemetry::trace::Span as _;
use opentelemetry::{Context, KeyValue};
use opentelemetry_sdk::error::OTelSdkResult;
use opentelemetry_sdk::trace::{Span, SpanData, SpanProcessor};

/// OpenTelemetry span processor that copies OTel baggage entries onto each
/// started span as attributes.
#[derive(Debug, Clone, Default)]
pub struct BaggageSpanProcessor;

impl BaggageSpanProcessor {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl SpanProcessor for BaggageSpanProcessor {
    fn on_start(&self, span: &mut Span, cx: &Context) {
        // NoOp guard: skip allocation when sampler drops the span.
        if !span.is_recording() {
            return;
        }

        for (key, (value, _meta)) in cx.baggage().iter() {
            span.set_attribute(KeyValue::new(
                key.as_str().to_string(),
                value.as_str().to_string(),
            ));
        }
    }

    fn on_end(&self, _span: SpanData) {}

    fn force_flush(&self) -> OTelSdkResult {
        Ok(())
    }

    fn shutdown_with_timeout(&self, _timeout: std::time::Duration) -> OTelSdkResult {
        Ok(())
    }

    fn shutdown(&self) -> OTelSdkResult {
        self.shutdown_with_timeout(std::time::Duration::from_secs(5))
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use opentelemetry::baggage::BaggageExt;
    use opentelemetry::trace::{Tracer, TracerProvider};
    use opentelemetry::{Context, KeyValue};
    use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider, SimpleSpanProcessor};

    fn build_test_provider(processor: BaggageSpanProcessor) -> (impl Tracer, InMemorySpanExporter) {
        let exporter = InMemorySpanExporter::default();
        let provider = SdkTracerProvider::builder()
            .with_span_processor(processor)
            .with_span_processor(SimpleSpanProcessor::new(exporter.clone()))
            .build();
        let tracer = provider.tracer("test");
        (tracer, exporter)
    }

    fn first_span_attr(exporter: &InMemorySpanExporter, key: &str) -> Option<String> {
        let spans = exporter.get_finished_spans().expect("exporter ok");
        spans.first().and_then(|s| {
            s.attributes
                .iter()
                .find(|kv| kv.key.as_str() == key)
                .map(|kv| kv.value.as_str().to_string())
        })
    }

    #[test]
    fn copies_every_baggage_entry_to_attributes() {
        let (tracer, exporter) = build_test_provider(BaggageSpanProcessor);

        let cx = Context::new().with_baggage(vec![
            KeyValue::new("iii.session.id", "S-1"),
            KeyValue::new("iii.session.name", "refactor auth"),
            KeyValue::new("iii.message.id", "M-1"),
            KeyValue::new("iii.function.id", "auth::set_token"),
            KeyValue::new("iii.tag.message", "fix the login bug"),
            // No key policy: non-iii baggage is a first-class tag source too.
            KeyValue::new("tenant.id", "t-42"),
        ]);

        let span = tracer
            .span_builder("inner")
            .start_with_context(&tracer, &cx);
        drop(span);

        for (key, expected) in [
            ("iii.session.id", "S-1"),
            ("iii.session.name", "refactor auth"),
            ("iii.message.id", "M-1"),
            ("iii.function.id", "auth::set_token"),
            ("iii.tag.message", "fix the login bug"),
            ("tenant.id", "t-42"),
        ] {
            assert_eq!(
                first_span_attr(&exporter, key).as_deref(),
                Some(expected),
                "baggage key {key} must be copied",
            );
        }
    }

    #[test]
    fn missing_baggage_entry_means_attribute_not_set() {
        let (tracer, exporter) = build_test_provider(BaggageSpanProcessor);

        let cx = Context::new().with_baggage(vec![KeyValue::new("iii.message.id", "M-only")]);

        let span = tracer
            .span_builder("inner")
            .start_with_context(&tracer, &cx);
        drop(span);

        assert_eq!(
            first_span_attr(&exporter, "iii.message.id").as_deref(),
            Some("M-only"),
        );
        assert!(first_span_attr(&exporter, "iii.session.id").is_none());
    }

    #[test]
    fn empty_parent_context_produces_no_attributes() {
        let (tracer, exporter) = build_test_provider(BaggageSpanProcessor);

        let span = tracer.start("inner");
        drop(span);

        assert!(first_span_attr(&exporter, "iii.session.id").is_none());
        assert!(first_span_attr(&exporter, "iii.message.id").is_none());
    }

    #[test]
    fn noop_guard_skips_processing_when_sampled_out() {
        let exporter = InMemorySpanExporter::default();
        let provider = SdkTracerProvider::builder()
            .with_sampler(opentelemetry_sdk::trace::Sampler::AlwaysOff)
            .with_span_processor(BaggageSpanProcessor)
            .with_span_processor(SimpleSpanProcessor::new(exporter.clone()))
            .build();
        let tracer = provider.tracer("test");

        let cx = Context::new().with_baggage(vec![
            KeyValue::new("iii.session.id", "S-1"),
            KeyValue::new("iii.message.id", "M-1"),
        ]);

        let span = tracer
            .span_builder("inner")
            .start_with_context(&tracer, &cx);
        drop(span);

        let spans = exporter.get_finished_spans().expect("exporter ok");
        assert!(
            spans.is_empty(),
            "AlwaysOff sampler should drop the span; no export expected"
        );
    }
}
