package observability

import (
	"context"
	"fmt"
	"net/url"
	"os"
	"sync"

	"go.opentelemetry.io/otel/exporters/otlp/otlplog/otlploghttp"
	"go.opentelemetry.io/otel/exporters/stdout/stdoutlog"
	otellog "go.opentelemetry.io/otel/log"
	"go.opentelemetry.io/otel/sdk/log"
	"go.opentelemetry.io/otel/sdk/resource"
	semconv "go.opentelemetry.io/otel/semconv/v1.41.0"
)

// ExporterKind selects where telemetry is sent. Mirrors the exporter choice in the Rust
// and Node observability packages (memory/stdout vs OTLP).
type ExporterKind string

const (
	// ExporterStdout writes log records to stdout (good for local dev / CI).
	ExporterStdout ExporterKind = "stdout"
	// ExporterOTLP ships log records to an OTLP/HTTP endpoint (a collector or backend).
	ExporterOTLP ExporterKind = "otlp"
)

// OtelConfig configures Init. The zero value (used by DefaultConfig) is a stdout
// exporter with service name "iii", matching the default-config behavior of the engine
// and the other SDKs.
type OtelConfig struct {
	// ServiceName is reported as the OTel service.name resource attribute.
	ServiceName string
	// Exporter selects stdout (default) or OTLP.
	Exporter ExporterKind
	// OTLPEndpoint is the OTLP/HTTP endpoint when Exporter is ExporterOTLP. If empty,
	// the standard OTEL_EXPORTER_OTLP_ENDPOINT env var is honored by the exporter.
	OTLPEndpoint string
}

// DefaultConfig returns the default telemetry configuration: stdout exporter, service
// name "iii".
func DefaultConfig() OtelConfig {
	return OtelConfig{ServiceName: "iii", Exporter: ExporterStdout}
}

var (
	mu             sync.Mutex
	loggerProvider *log.LoggerProvider
)

// Init wires an OpenTelemetry logger provider per cfg and installs it as this package's
// provider, so Logger emits real OTel LogRecords. It returns a shutdown function that
// flushes and tears down the provider; call it before the process exits. Init is
// idempotent — a second call shuts the previous provider down first.
//
// Mirrors the init plumbing in sdk/packages/rust/observability (OtelConfig::default +
// provider setup). Trace/metrics exporters are a planned next layer; v1 wires logs,
// which is what the Logger needs.
func Init(ctx context.Context, cfg OtelConfig) (shutdown func(context.Context) error, err error) {
	if cfg.ServiceName == "" {
		cfg.ServiceName = "iii"
	}

	exporter, err := newLogExporter(ctx, cfg)
	if err != nil {
		return nil, err
	}

	res, err := resource.Merge(resource.Default(), resource.NewWithAttributes(
		semconv.SchemaURL,
		semconv.ServiceName(cfg.ServiceName),
	))
	if err != nil {
		return nil, fmt.Errorf("observability: building resource: %w", err)
	}

	provider := log.NewLoggerProvider(
		log.WithProcessor(log.NewBatchProcessor(exporter)),
		log.WithResource(res),
	)

	mu.Lock()
	prev := loggerProvider
	loggerProvider = provider
	mu.Unlock()
	if prev != nil {
		_ = prev.Shutdown(ctx)
	}

	return func(ctx context.Context) error {
		mu.Lock()
		if loggerProvider == provider {
			loggerProvider = nil
		}
		mu.Unlock()
		return provider.Shutdown(ctx)
	}, nil
}

func newLogExporter(ctx context.Context, cfg OtelConfig) (log.Exporter, error) {
	switch cfg.Exporter {
	case ExporterOTLP:
		opts, err := otlpOptions(cfg.OTLPEndpoint)
		if err != nil {
			return nil, err
		}
		exp, err := otlploghttp.New(ctx, opts...)
		if err != nil {
			return nil, fmt.Errorf("observability: creating OTLP log exporter: %w", err)
		}
		return exp, nil
	case ExporterStdout, "":
		exp, err := stdoutlog.New(stdoutlog.WithWriter(os.Stdout))
		if err != nil {
			return nil, fmt.Errorf("observability: creating stdout log exporter: %w", err)
		}
		return exp, nil
	default:
		return nil, fmt.Errorf("observability: unknown exporter %q", cfg.Exporter)
	}
}

// otlpOptions turns a user-supplied endpoint (e.g. "http://localhost:4318") into
// otlploghttp options. It passes the host:port via WithEndpoint — which appends the
// standard "/v1/logs" path — rather than WithEndpointURL, so a base URL without an
// explicit signal path still reaches the right endpoint. An http:// scheme selects an
// insecure (plaintext) connection. An empty endpoint leaves the exporter to honor the
// standard OTEL_EXPORTER_OTLP_* env vars.
func otlpOptions(endpoint string) ([]otlploghttp.Option, error) {
	if endpoint == "" {
		return nil, nil
	}
	u, err := url.Parse(endpoint)
	if err != nil {
		return nil, fmt.Errorf("observability: invalid OTLP endpoint %q: %w", endpoint, err)
	}
	// If the caller already included a signal path, honor the full URL as-is.
	if u.Path != "" && u.Path != "/" {
		return []otlploghttp.Option{otlploghttp.WithEndpointURL(endpoint)}, nil
	}
	opts := []otlploghttp.Option{otlploghttp.WithEndpoint(u.Host)}
	if u.Scheme == "http" {
		opts = append(opts, otlploghttp.WithInsecure())
	}
	return opts, nil
}

// getLoggerProvider returns the installed OTel logger provider, or nil if Init has not
// run. Logger uses this to decide whether to emit OTel records or fall back. Mirrors
// telemetry::get_logger_provider in the Rust package.
func getLoggerProvider() otellog.LoggerProvider {
	mu.Lock()
	defer mu.Unlock()
	if loggerProvider == nil {
		return nil
	}
	return loggerProvider
}
