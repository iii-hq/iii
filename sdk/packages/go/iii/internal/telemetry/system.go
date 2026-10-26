package telemetry

import (
	"context"
	"errors"
	"sync"
	"sync/atomic"

	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/attribute"
	otellog "go.opentelemetry.io/otel/log"
	"go.opentelemetry.io/otel/metric"
	sdklog "go.opentelemetry.io/otel/sdk/log"
	sdkmetric "go.opentelemetry.io/otel/sdk/metric"
	"go.opentelemetry.io/otel/sdk/resource"
	sdktrace "go.opentelemetry.io/otel/sdk/trace"
	"go.opentelemetry.io/otel/trace"
)

// Resource attribute keys (Node ATTR_SERVICE_*).
const (
	attrServiceName       = "service.name"
	attrServiceVersion    = "service.version"
	attrServiceNamespace  = "service.namespace"
	attrServiceInstanceID = "service.instance.id"
)

// active is the most recently started System: what the package-level helpers (Logger,
// WithSpan, the HTTP transport, Flush) use, as the Node helpers use their module state.
var active atomic.Pointer[System]

// Active returns the running System started last, or nil.
func Active() *System { return active.Load() }

// System is one running telemetry pipeline (Node initOtel's module state): a shared
// connection, a tracer provider with the baggage and batch span processors, an optional
// meter provider with a periodic reader, and a logger provider with a batch processor.
type System struct {
	cfg    Config
	conn   *Connection
	tp     *sdktrace.TracerProvider
	mp     *sdkmetric.MeterProvider
	lp     *sdklog.LoggerProvider
	tracer trace.Tracer
	meter  metric.Meter
	logger otellog.Logger

	gaugesMu sync.Mutex
	gauges   *WorkerGauges

	shutdownOnce sync.Once
}

// Start builds and registers a System for cfg, which must be enabled and resolved. Like
// Node initOtel it registers the W3C propagator, the tracer provider and (when metrics
// are enabled) the meter provider as the OpenTelemetry globals, so spans and metrics a
// handler creates through the otel package reach the engine too.
func Start(cfg Config) (*System, error) {
	if !cfg.Enabled {
		return nil, errors.New("telemetry: disabled")
	}
	if cfg.Endpoint == "" {
		endpoint, err := AppendOtelPath(cfg.EngineWSURL)
		if err != nil {
			return nil, err
		}
		cfg.Endpoint = endpoint
	}

	attrs := []attribute.KeyValue{
		attribute.String(attrServiceName, cfg.ServiceName),
		attribute.String(attrServiceVersion, cfg.ServiceVersion),
		attribute.String(attrServiceInstanceID, cfg.ServiceInstanceID),
	}
	if cfg.ServiceNamespace != "" {
		attrs = append(attrs, attribute.String(attrServiceNamespace, cfg.ServiceNamespace))
	}
	res := resource.NewSchemaless(attrs...)

	s := &System{cfg: cfg}
	s.conn = NewConnection(cfg.Endpoint, cfg.EngineWSHeaders, cfg.Reconnection)

	// The baggage processor registers first: OnStart runs in registration order, so
	// baggage is on the span before the batch processor reads it.
	s.tp = sdktrace.NewTracerProvider(
		sdktrace.WithResource(res),
		sdktrace.WithSpanProcessor(BaggageSpanProcessor{}),
		sdktrace.WithBatcher(NewSpanExporter(s.conn), sdktrace.WithBatchTimeout(cfg.SpansFlushInterval)),
	)
	s.tracer = s.tp.Tracer(cfg.ServiceName)
	otel.SetTextMapPropagator(Propagator)
	otel.SetTracerProvider(s.tp)

	if cfg.MetricsEnabled {
		reader := sdkmetric.NewPeriodicReader(NewMetricExporter(s.conn), sdkmetric.WithInterval(cfg.MetricsExportInterval))
		s.mp = sdkmetric.NewMeterProvider(sdkmetric.WithResource(res), sdkmetric.WithReader(reader))
		s.meter = s.mp.Meter(cfg.ServiceName)
		otel.SetMeterProvider(s.mp)
	}

	// Logs are always on when telemetry is on, as in the Node SDK.
	s.lp = sdklog.NewLoggerProvider(
		sdklog.WithResource(res),
		sdklog.WithProcessor(sdklog.NewBatchProcessor(
			NewLogExporter(s.conn),
			sdklog.WithExportInterval(cfg.LogsFlushInterval),
			sdklog.WithExportMaxBatchSize(cfg.LogsBatchSize),
		)),
	)
	s.logger = s.lp.Logger(cfg.ServiceName)

	active.Store(s)
	return s, nil
}

// Config returns the resolved configuration.
func (s *System) Config() Config { return s.cfg }

// Tracer returns the System's tracer (scope: the service name).
func (s *System) Tracer() trace.Tracer { return s.tracer }

// Meter returns the System's meter, nil when metrics are disabled.
func (s *System) Meter() metric.Meter { return s.meter }

// Logger returns the System's OpenTelemetry logger.
func (s *System) Logger() otellog.Logger { return s.logger }

// Connection returns the System's engine connection.
func (s *System) Connection() *Connection { return s.conn }

// StartWorkerGauges registers the worker gauges once (Node registerWorkerGauges guard);
// a no-op when metrics are disabled or the gauges already run.
func (s *System) StartWorkerGauges(opts WorkerGaugesOptions) error {
	if s.meter == nil {
		return errors.New("telemetry: metrics are disabled")
	}
	s.gaugesMu.Lock()
	defer s.gaugesMu.Unlock()
	if s.gauges != nil {
		return nil
	}
	g, err := RegisterWorkerGauges(s.meter, opts)
	if err != nil {
		return err
	}
	s.gauges = g
	return nil
}

// StopWorkerGauges unregisters the worker gauges (Node stopWorkerGauges).
func (s *System) StopWorkerGauges() {
	s.gaugesMu.Lock()
	g := s.gauges
	s.gauges = nil
	s.gaugesMu.Unlock()
	if g != nil {
		_ = g.Stop()
	}
}

// Flush force-flushes every provider without tearing them down (Node flushOtel).
func (s *System) Flush(ctx context.Context) error {
	var errs []error
	errs = append(errs, s.tp.ForceFlush(ctx))
	if s.mp != nil {
		errs = append(errs, s.mp.ForceFlush(ctx))
	}
	errs = append(errs, s.lp.ForceFlush(ctx))
	return errors.Join(errs...)
}

// Shutdown flushes and stops the pipeline (Node shutdownOtel): the connection stops
// reconnecting first so a dead engine cannot hold the final flush, then each provider
// is flushed and shut down (errors are best effort and ignored), then the socket is
// closed. It is idempotent and bounded by ctx.
func (s *System) Shutdown(ctx context.Context) error {
	var err error
	s.shutdownOnce.Do(func() {
		s.StopWorkerGauges()
		s.conn.BeginShutdown()
		_ = s.tp.ForceFlush(ctx)
		_ = s.tp.Shutdown(ctx)
		if s.mp != nil {
			_ = s.mp.ForceFlush(ctx)
			_ = s.mp.Shutdown(ctx)
		}
		_ = s.lp.ForceFlush(ctx)
		_ = s.lp.Shutdown(ctx)
		err = s.conn.Shutdown(ctx)
		active.CompareAndSwap(s, nil)
	})
	return err
}
