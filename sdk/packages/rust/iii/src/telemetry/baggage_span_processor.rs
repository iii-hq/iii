//! Baggage -> span attribute processor.

use opentelemetry::baggage::BaggageExt;
use opentelemetry::trace::Span as _;
use opentelemetry::{Context, KeyValue};
use opentelemetry_sdk::error::OTelSdkResult;
use opentelemetry_sdk::trace::{Span, SpanData, SpanProcessor};

/// DEFAULT_ALLOWLIST drift across languages would break worker chains;
/// lockstep tests in each SDK pin this constant at CI time.
pub const DEFAULT_ALLOWLIST: &[&str] =
    &["iii.session.id", "iii.message.id", "iii.function_id"];

#[derive(Debug, Clone)]
pub struct BaggageSpanProcessor {
    allowlist: Vec<&'static str>,
}

impl BaggageSpanProcessor {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub const fn with_allowlist(keys: Vec<&'static str>) -> Self {
        Self { allowlist: keys }
    }
}

impl Default for BaggageSpanProcessor {
    fn default() -> Self {
        Self {
            allowlist: DEFAULT_ALLOWLIST.to_vec(),
        }
    }
}

impl SpanProcessor for BaggageSpanProcessor {
    fn on_start(&self, span: &mut Span, cx: &Context) {
        // NoOp guard: skip allocation when sampler drops the span.
        if !span.is_recording() {
            return;
        }

        let baggage = cx.baggage();
        for key in &self.allowlist {
            if let Some(value) = baggage.get(*key) {
                span.set_attribute(KeyValue::new(
                    (*key).to_string(),
                    value.as_str().to_string(),
                ));
            }
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

    fn build_test_provider(
        processor: BaggageSpanProcessor,
    ) -> (impl Tracer, InMemorySpanExporter) {
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
    fn copies_default_allowlist_from_baggage_to_attributes() {
        let (tracer, exporter) = build_test_provider(BaggageSpanProcessor::default());

        let cx = Context::new().with_baggage(vec![
            KeyValue::new("iii.session.id", "S-1"),
            KeyValue::new("iii.message.id", "M-1"),
            KeyValue::new("iii.function_id", "auth::set_token"),
        ]);

        let span = tracer.span_builder("inner").start_with_context(&tracer, &cx);
        drop(span);

        assert_eq!(
            first_span_attr(&exporter, "iii.session.id").as_deref(),
            Some("S-1"),
        );
        assert_eq!(
            first_span_attr(&exporter, "iii.message.id").as_deref(),
            Some("M-1"),
        );
        assert_eq!(
            first_span_attr(&exporter, "iii.function_id").as_deref(),
            Some("auth::set_token"),
        );
    }

    #[test]
    fn missing_baggage_entry_means_attribute_not_set() {
        let (tracer, exporter) = build_test_provider(BaggageSpanProcessor::default());

        let cx = Context::new()
            .with_baggage(vec![KeyValue::new("iii.message.id", "M-only")]);

        let span = tracer.span_builder("inner").start_with_context(&tracer, &cx);
        drop(span);

        assert_eq!(
            first_span_attr(&exporter, "iii.message.id").as_deref(),
            Some("M-only"),
        );
        assert!(first_span_attr(&exporter, "iii.session.id").is_none());
        assert!(first_span_attr(&exporter, "iii.function_id").is_none());
    }

    #[test]
    fn baggage_entries_not_in_allowlist_are_dropped() {
        let (tracer, exporter) = build_test_provider(BaggageSpanProcessor::default());

        let cx = Context::new().with_baggage(vec![
            KeyValue::new("iii.message.id", "M"),
            KeyValue::new("tenant.id", "t-42"),
            KeyValue::new("debug.feature_flag", "on"),
        ]);

        let span = tracer.span_builder("inner").start_with_context(&tracer, &cx);
        drop(span);

        assert_eq!(
            first_span_attr(&exporter, "iii.message.id").as_deref(),
            Some("M"),
        );
        assert!(first_span_attr(&exporter, "tenant.id").is_none());
        assert!(first_span_attr(&exporter, "debug.feature_flag").is_none());
    }

    #[test]
    fn custom_allowlist_is_honored() {
        let processor =
            BaggageSpanProcessor::with_allowlist(vec!["tenant.id", "iii.message.id"]);
        let (tracer, exporter) = build_test_provider(processor);

        let cx = Context::new().with_baggage(vec![
            KeyValue::new("tenant.id", "t-1"),
            KeyValue::new("iii.message.id", "M"),
            KeyValue::new("iii.session.id", "S-not-copied"),
        ]);

        let span = tracer.span_builder("inner").start_with_context(&tracer, &cx);
        drop(span);

        assert_eq!(
            first_span_attr(&exporter, "tenant.id").as_deref(),
            Some("t-1"),
        );
        assert_eq!(
            first_span_attr(&exporter, "iii.message.id").as_deref(),
            Some("M"),
        );
        assert!(first_span_attr(&exporter, "iii.session.id").is_none());
    }

    #[test]
    fn empty_parent_context_produces_no_attributes() {
        let (tracer, exporter) = build_test_provider(BaggageSpanProcessor::default());

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
            .with_span_processor(BaggageSpanProcessor::default())
            .with_span_processor(SimpleSpanProcessor::new(exporter.clone()))
            .build();
        let tracer = provider.tracer("test");

        let cx = Context::new().with_baggage(vec![
            KeyValue::new("iii.session.id", "S-1"),
            KeyValue::new("iii.message.id", "M-1"),
        ]);

        let span = tracer.span_builder("inner").start_with_context(&tracer, &cx);
        drop(span);

        let spans = exporter.get_finished_spans().expect("exporter ok");
        assert!(
            spans.is_empty(),
            "AlwaysOff sampler should drop the span; no export expected"
        );
    }

    #[test]
    fn default_allowlist_matches_harness_baggage_write_set() {
        // DEFAULT_ALLOWLIST drift across languages would break worker chains.
        assert_eq!(
            DEFAULT_ALLOWLIST,
            &["iii.session.id", "iii.message.id", "iii.function_id"],
        );
    }
}
