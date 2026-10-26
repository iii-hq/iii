package iii

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log"
	"sync"
	"time"

	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/codes"
	"go.opentelemetry.io/otel/trace"

	"github.com/iii-hq/iii/sdk/packages/go/iii/internal/telemetry"
)

// otelShutdownTimeout bounds the final flush in Close (the Rust SDK's default
// shutdown_timeout_ms).
const otelShutdownTimeout = 10 * time.Second

// otelState is a client's telemetry: the configuration its options set and the running
// pipeline. Guarded by mu.
type otelState struct {
	mu                       sync.Mutex
	config                   OtelConfig
	metricsReportingDisabled bool
	started                  bool
	system                   *telemetry.System
	gaugesWarned             bool
}

// telemetryOptions maps the public OtelConfig onto the internal options, adding the
// engine address and handshake headers the client owns.
func (c *Client) telemetryOptions(cfg OtelConfig) telemetry.Options {
	opts := telemetry.Options{
		Enabled:                     cfg.Enabled,
		ServiceName:                 cfg.ServiceName,
		ServiceVersion:              cfg.ServiceVersion,
		ServiceNamespace:            cfg.ServiceNamespace,
		ServiceInstanceID:           cfg.ServiceInstanceID,
		EngineWSURL:                 c.url,
		EngineWSHeaders:             handshakeHeaders(c.headers),
		MetricsEnabled:              cfg.MetricsEnabled,
		MetricsExportInterval:       cfg.MetricsExportInterval,
		SpansFlushInterval:          cfg.SpansFlushInterval,
		LogsFlushInterval:           cfg.LogsFlushInterval,
		LogsBatchSize:               cfg.LogsBatchSize,
		FetchInstrumentationEnabled: cfg.FetchInstrumentationEnabled,
	}
	if r := cfg.ReconnectionConfig; r != nil {
		opts.Reconnection = &telemetry.Reconnect{
			InitialDelay:      r.InitialDelay,
			MaxDelay:          r.MaxDelay,
			BackoffMultiplier: r.BackoffMultiplier,
			JitterFactor:      r.JitterFactor,
			MaxRetries:        r.MaxRetries,
		}
	}
	return opts
}

// startTelemetry starts the client's pipeline once, when the connection lifecycle
// starts (Node initializes OTel in the iii constructor). Disabled or misconfigured
// telemetry leaves the client without one; nothing is dialled then.
func (c *Client) startTelemetry() {
	select {
	case <-c.shutdown:
		return
	default:
	}
	s := &c.otel
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.started {
		return
	}
	s.started = true
	cfg, err := telemetry.Resolve(c.telemetryOptions(s.config), nil)
	if err != nil {
		log.Printf("iii: OpenTelemetry not started: %v", err)
		return
	}
	if !cfg.Enabled {
		return
	}
	sys, err := telemetry.Start(cfg)
	if err != nil {
		log.Printf("iii: OpenTelemetry not started: %v", err)
		return
	}
	s.system = sys
}

// stopTelemetry stops the worker gauges, then flushes and shuts the pipeline down
// (Node shutdown: stopMetricsReporting, then shutdownOtel), bounded by
// otelShutdownTimeout.
func (c *Client) stopTelemetry() {
	s := &c.otel
	s.mu.Lock()
	sys := s.system
	s.system = nil
	s.mu.Unlock()
	if sys == nil {
		return
	}
	sys.StopWorkerGauges()
	ctx, cancel := context.WithTimeout(context.Background(), otelShutdownTimeout)
	defer cancel()
	_ = sys.Shutdown(ctx)
}

// telemetrySystem returns the running pipeline, nil when telemetry is off.
func (c *Client) telemetrySystem() *telemetry.System {
	c.otel.mu.Lock()
	defer c.otel.mu.Unlock()
	return c.otel.system
}

// startMetricsReporting registers the worker gauges once the engine assigned this
// worker its id (Node startMetricsReporting on WorkerRegistered).
func (c *Client) startMetricsReporting() {
	s := &c.otel
	s.mu.Lock()
	sys, disabled := s.system, s.metricsReportingDisabled
	s.mu.Unlock()
	if disabled || sys == nil {
		return
	}
	c.mu.Lock()
	workerID, name := c.workerID, c.name
	c.mu.Unlock()
	if workerID == "" {
		return
	}
	if err := sys.StartWorkerGauges(telemetry.WorkerGaugesOptions{WorkerID: workerID, WorkerName: name}); err != nil {
		s.mu.Lock()
		warn := !s.gaugesWarned
		s.gaugesWarned = true
		s.mu.Unlock()
		if warn {
			log.Printf("iii: worker metrics disabled: %v", err)
		}
	}
}

// stopMetricsReporting unregisters the worker gauges (Node stopMetricsReporting).
func (c *Client) stopMetricsReporting() {
	if sys := c.telemetrySystem(); sys != nil {
		sys.StopWorkerGauges()
	}
}

// startInvocationSpan starts the span around a handler, as the Node SDK's handler
// wrapper does: named "execute <function id>" (the engine already emits the "call" and
// "trigger" spans), kind INTERNAL, a child of the trace context in ctx, with the
// redacted input and output recorded as iii.invocation.input / iii.invocation.output
// events unless III_DISABLE_TRACE_PAYLOADS is set (capped by
// III_TRACE_PAYLOAD_MAX_BYTES). The returned function ends the span with the handler's
// outcome: OK, or ERROR with an exception event. Without running telemetry both are
// no-ops and ctx is returned unchanged.
func (c *Client) startInvocationSpan(ctx context.Context, input json.RawMessage) (context.Context, func(result any, err error)) {
	sys := c.telemetrySystem()
	if sys == nil {
		return ctx, func(any, error) {}
	}
	functionID := ""
	if inv := invocationFrom(ctx); inv != nil {
		functionID = inv.functionID
	}
	ctx, span := sys.Tracer().Start(ctx, "execute "+functionID, trace.WithSpanKind(trace.SpanKindInternal))

	tracePayloads := span.IsRecording() && telemetry.TracePayloadsEnabled()
	maxBytes := 0
	if tracePayloads {
		maxBytes = telemetry.ResolveMaxBytesFromEnv()
		payload, truncated := telemetry.RedactAndTruncate(input, maxBytes)
		span.AddEvent("iii.invocation.input", trace.WithAttributes(
			attribute.String("iii.payload.json", payload),
			attribute.Bool("iii.payload.truncated", truncated),
		))
	}

	return ctx, func(result any, err error) {
		if tracePayloads {
			value := result
			if err != nil {
				value = map[string]string{"error": invocationErrorMessage(err)}
			}
			payload, truncated := telemetry.RedactAndTruncate(value, maxBytes)
			span.AddEvent("iii.invocation.output", trace.WithAttributes(
				attribute.String("iii.payload.json", payload),
				attribute.Bool("iii.payload.truncated", truncated),
				attribute.Bool("iii.payload.ok", err == nil),
			))
		}
		if err != nil {
			span.SetStatus(codes.Error, invocationErrorMessage(err))
			span.AddEvent("exception", trace.WithAttributes(exceptionAttributes(err)...))
		} else {
			span.SetStatus(codes.Ok, "")
		}
		span.End()
	}
}

// invocationErrorMessage is the message of a handler error (Node error.message): an
// *InvocationError's own message, else err.Error().
func invocationErrorMessage(err error) string {
	var ie *InvocationError
	if errors.As(err, &ie) {
		return ie.Message
	}
	return err.Error()
}

// exceptionAttributes are the OpenTelemetry exception event attributes for err (Node
// span.recordException; like the Rust SDK an *InvocationError reports its code as the
// type and carries its stacktrace).
func exceptionAttributes(err error) []attribute.KeyValue {
	var ie *InvocationError
	if errors.As(err, &ie) {
		attrs := []attribute.KeyValue{
			attribute.String("exception.type", ie.Code),
			attribute.String("exception.message", ie.Message),
		}
		if ie.Stacktrace != "" {
			attrs = append(attrs, attribute.String("exception.stacktrace", ie.Stacktrace))
		}
		return attrs
	}
	return []attribute.KeyValue{
		attribute.String("exception.type", fmt.Sprintf("%T", err)),
		attribute.String("exception.message", err.Error()),
	}
}
