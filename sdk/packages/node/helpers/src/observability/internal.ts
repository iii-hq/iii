// Internal entry point: primitives shared with other first-party iii packages
// (e.g. @iii-dev/iii) but intentionally kept out of the public API surface.
// External consumers should use withSpan/initOtel and import SpanKind from
// @opentelemetry/api directly.
export { getMeter, getTracer } from './telemetry-system'
