// Package telemetry is the OpenTelemetry implementation behind the iii Go SDK. It
// mirrors the Node SDK's observability/telemetry-system
// (sdk/packages/node/helpers/src/observability/telemetry-system): one WebSocket to the
// engine's /otel endpoint carries spans, metrics and logs as binary frames, each a
// four-byte signal prefix (OTLP, MTRC, LOGS) followed by an OTLP/JSON document shaped
// exactly like @opentelemetry/otlp-transformer's JSON serializers.
//
// The package is internal: package iii starts and stops a System per Client, and
// package observability exposes the user-facing helpers.
package telemetry

import (
	"fmt"
	"math"
	"net/http"
	"net/url"
	"os"
	"strconv"
	"strings"
	"time"

	"github.com/google/uuid"
)

// Signal prefixes of the binary frames sent to the engine (Node telemetry-system/types.ts
// PREFIX_TRACES, PREFIX_METRICS, PREFIX_LOGS).
const (
	PrefixTraces  = "OTLP"
	PrefixMetrics = "MTRC"
	PrefixLogs    = "LOGS"
)

// Defaults mirror DEFAULT_OTEL_CONFIG (Node telemetry-system/types.ts). The service
// name is the Go counterpart of the Node default "iii-node".
const (
	DefaultServiceName           = "iii-go"
	DefaultServiceVersion        = "unknown"
	DefaultEngineWSURL           = "ws://localhost:49134"
	DefaultMetricsExportInterval = 60 * time.Second
	DefaultSpansFlushInterval    = 100 * time.Millisecond
	DefaultLogsFlushInterval     = 100 * time.Millisecond
	DefaultLogsBatchSize         = 1
)

// minFlushInterval keeps a batch processor from spinning: the Node SDK accepts a 0ms
// delay, but the Go batch processors re-arm their timer with it in a loop.
const minFlushInterval = time.Millisecond

// Reconnect is the backoff schedule of the telemetry WebSocket (Node ReconnectionConfig).
type Reconnect struct {
	InitialDelay      time.Duration
	MaxDelay          time.Duration
	BackoffMultiplier float64
	JitterFactor      float64
	// MaxRetries caps reconnect attempts; -1 retries forever.
	MaxRetries int
}

// DefaultReconnect is DEFAULT_RECONNECTION_CONFIG: 1s, x2, capped at 30s, 30% jitter,
// retry forever.
func DefaultReconnect() Reconnect {
	return Reconnect{
		InitialDelay:      time.Second,
		MaxDelay:          30 * time.Second,
		BackoffMultiplier: 2,
		JitterFactor:      0.3,
		MaxRetries:        -1,
	}
}

// Options is the configuration before environment resolution. A nil pointer, an empty
// string or a zero duration/size means "not set": the environment, then the default,
// fills it in, as Node initOtel does with `config.x ?? process.env.X ?? DEFAULT`.
type Options struct {
	Enabled                     *bool
	ServiceName                 string
	ServiceVersion              string
	ServiceNamespace            string
	ServiceInstanceID           string
	EngineWSURL                 string
	EngineWSHeaders             http.Header
	MetricsEnabled              *bool
	MetricsExportInterval       time.Duration
	SpansFlushInterval          time.Duration
	LogsFlushInterval           time.Duration
	LogsBatchSize               int
	FetchInstrumentationEnabled *bool
	Reconnection                *Reconnect
}

// Config is a fully resolved configuration.
type Config struct {
	Enabled           bool
	ServiceName       string
	ServiceVersion    string
	ServiceNamespace  string
	ServiceInstanceID string
	// EngineWSURL is the engine address as configured; Endpoint is the /otel URL the
	// telemetry socket dials.
	EngineWSURL                 string
	Endpoint                    string
	EngineWSHeaders             http.Header
	MetricsEnabled              bool
	MetricsExportInterval       time.Duration
	SpansFlushInterval          time.Duration
	LogsFlushInterval           time.Duration
	LogsBatchSize               int
	FetchInstrumentationEnabled bool
	Reconnection                Reconnect
}

// LookupEnv reads an environment variable; os.LookupEnv in production, a map in tests.
type LookupEnv func(key string) (string, bool)

// Resolve fills o from the environment and the defaults, in Node initOtel's order. It
// only fails when telemetry is enabled and the engine URL cannot be parsed.
func Resolve(o Options, env LookupEnv) (Config, error) {
	if env == nil {
		env = os.LookupEnv
	}
	cfg := Config{
		Enabled:                     boolOr(o.Enabled, func() bool { return parseBoolEnv(env, "OTEL_ENABLED", true) }),
		ServiceName:                 firstNonBlank(o.ServiceName, envString(env, "OTEL_SERVICE_NAME"), DefaultServiceName),
		ServiceVersion:              firstNonBlank(o.ServiceVersion, envString(env, "SERVICE_VERSION"), DefaultServiceVersion),
		ServiceNamespace:            firstNonBlank(o.ServiceNamespace, envString(env, "SERVICE_NAMESPACE")),
		ServiceInstanceID:           firstNonBlank(o.ServiceInstanceID, envString(env, "SERVICE_INSTANCE_ID")),
		EngineWSURL:                 firstNonBlank(o.EngineWSURL, envString(env, "III_URL"), DefaultEngineWSURL),
		EngineWSHeaders:             o.EngineWSHeaders.Clone(),
		MetricsEnabled:              boolOr(o.MetricsEnabled, func() bool { return parseBoolEnv(env, "OTEL_METRICS_ENABLED", true) }),
		MetricsExportInterval:       o.MetricsExportInterval,
		SpansFlushInterval:          resolveFlushInterval(o.SpansFlushInterval, env, "OTEL_SPANS_FLUSH_INTERVAL_MS", DefaultSpansFlushInterval),
		LogsFlushInterval:           resolveFlushInterval(o.LogsFlushInterval, env, "OTEL_LOGS_FLUSH_INTERVAL_MS", DefaultLogsFlushInterval),
		LogsBatchSize:               o.LogsBatchSize,
		FetchInstrumentationEnabled: boolOr(o.FetchInstrumentationEnabled, func() bool { return true }),
		Reconnection:                DefaultReconnect(),
	}
	if cfg.ServiceInstanceID == "" {
		cfg.ServiceInstanceID = uuid.NewString()
	}
	if cfg.MetricsExportInterval <= 0 {
		cfg.MetricsExportInterval = DefaultMetricsExportInterval
	}
	if cfg.LogsBatchSize <= 0 {
		if n, ok := parseIntegerEnv(env, "OTEL_LOGS_BATCH_SIZE", 1); ok {
			cfg.LogsBatchSize = n
		} else {
			cfg.LogsBatchSize = DefaultLogsBatchSize
		}
	}
	if o.Reconnection != nil {
		cfg.Reconnection = *o.Reconnection
	}
	if !cfg.Enabled {
		return cfg, nil
	}
	endpoint, err := AppendOtelPath(cfg.EngineWSURL)
	if err != nil {
		return cfg, err
	}
	cfg.Endpoint = endpoint
	return cfg, nil
}

// AppendOtelPath turns an engine WebSocket URL into its dedicated telemetry endpoint,
// like Node appendOtelPath: trailing slashes are trimmed and "/otel" is appended unless
// the path already ends with it; query and fragment are kept. The /otel endpoint keeps
// the telemetry socket out of the engine's worker registry.
func AppendOtelPath(base string) (string, error) {
	u, err := url.Parse(strings.TrimSpace(base))
	if err != nil {
		return "", fmt.Errorf("telemetry: invalid engine URL %q: %w", base, err)
	}
	if u.Scheme == "" || u.Host == "" {
		return "", fmt.Errorf("telemetry: invalid engine URL %q: scheme and host are required", base)
	}
	path := strings.TrimRight(u.Path, "/")
	if !strings.HasSuffix(path, "/otel") {
		path += "/otel"
	}
	u.Path = path
	u.RawPath = ""
	return u.String(), nil
}

// parseBoolEnv is Node parseBoolEnv: unset keeps the default; "false", "0", "no" and
// "off" (any case) are false; every other value, the empty string included, is true.
func parseBoolEnv(env LookupEnv, key string, def bool) bool {
	v, ok := env(key)
	if !ok {
		return def
	}
	switch strings.ToLower(v) {
	case "false", "0", "no", "off":
		return false
	}
	return true
}

// parseNumberEnv is Node parseNumberEnv: blank is unset; the value must be a finite
// number no smaller than minimum.
func parseNumberEnv(env LookupEnv, key string, minimum float64) (float64, bool) {
	v, ok := env(key)
	if !ok || strings.TrimSpace(v) == "" {
		return 0, false
	}
	f, err := strconv.ParseFloat(strings.TrimSpace(v), 64)
	if err != nil || math.IsNaN(f) || math.IsInf(f, 0) || f < minimum {
		return 0, false
	}
	return f, true
}

// parseIntegerEnv is Node parseIntegerEnv: parseNumberEnv restricted to integers.
func parseIntegerEnv(env LookupEnv, key string, minimum float64) (int, bool) {
	f, ok := parseNumberEnv(env, key, minimum)
	if !ok || f != math.Trunc(f) || f > math.MaxInt32 {
		return 0, false
	}
	return int(f), true
}

// resolveFlushInterval is Node resolveFlushIntervalMs: explicit config, then the III env
// var (milliseconds), then the default. Like the other iii SDKs it ignores the standard
// OTEL_BSP_SCHEDULE_DELAY / OTEL_BLRP_SCHEDULE_DELAY variables.
func resolveFlushInterval(configured time.Duration, env LookupEnv, key string, def time.Duration) time.Duration {
	d := def
	if configured > 0 {
		d = configured
	} else if ms, ok := parseNumberEnv(env, key, 0); ok {
		d = time.Duration(ms * float64(time.Millisecond))
	}
	if d < minFlushInterval {
		d = minFlushInterval
	}
	return d
}

func boolOr(v *bool, def func() bool) bool {
	if v != nil {
		return *v
	}
	return def()
}

// envString returns the trimmed value of key, "" when unset or blank.
func envString(env LookupEnv, key string) string {
	v, _ := env(key)
	return strings.TrimSpace(v)
}

func firstNonBlank(values ...string) string {
	for _, v := range values {
		if strings.TrimSpace(v) != "" {
			return v
		}
	}
	return ""
}
