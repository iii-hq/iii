package telemetry

import (
	"net/http"
	"testing"
	"time"
)

func envMap(m map[string]string) LookupEnv {
	return func(k string) (string, bool) {
		v, ok := m[k]
		return v, ok
	}
}

func boolPtr(b bool) *bool { return &b }

// Defaults mirror DEFAULT_OTEL_CONFIG / initOtel in the Node SDK.
func TestResolveDefaults(t *testing.T) {
	cfg, err := Resolve(Options{}, envMap(nil))
	if err != nil {
		t.Fatal(err)
	}
	if !cfg.Enabled || !cfg.MetricsEnabled || !cfg.FetchInstrumentationEnabled {
		t.Errorf("enabled=%v metrics=%v fetch=%v, want all true", cfg.Enabled, cfg.MetricsEnabled, cfg.FetchInstrumentationEnabled)
	}
	if cfg.ServiceName != "iii-go" || cfg.ServiceVersion != "unknown" || cfg.ServiceNamespace != "" {
		t.Errorf("service = %q %q %q", cfg.ServiceName, cfg.ServiceVersion, cfg.ServiceNamespace)
	}
	if len(cfg.ServiceInstanceID) != 36 {
		t.Errorf("service instance id %q is not a UUID", cfg.ServiceInstanceID)
	}
	if cfg.EngineWSURL != "ws://localhost:49134" || cfg.Endpoint != "ws://localhost:49134/otel" {
		t.Errorf("engine %q endpoint %q", cfg.EngineWSURL, cfg.Endpoint)
	}
	if cfg.MetricsExportInterval != 60*time.Second || cfg.SpansFlushInterval != 100*time.Millisecond ||
		cfg.LogsFlushInterval != 100*time.Millisecond || cfg.LogsBatchSize != 1 {
		t.Errorf("intervals %v %v %v batch %d", cfg.MetricsExportInterval, cfg.SpansFlushInterval, cfg.LogsFlushInterval, cfg.LogsBatchSize)
	}
	if cfg.Reconnection != DefaultReconnect() {
		t.Errorf("reconnection %+v", cfg.Reconnection)
	}
}

func TestResolveEnvironment(t *testing.T) {
	cfg, err := Resolve(Options{}, envMap(map[string]string{
		"OTEL_SERVICE_NAME":            "orders",
		"SERVICE_VERSION":              "2.0.0",
		"SERVICE_NAMESPACE":            "shop",
		"SERVICE_INSTANCE_ID":          "inst-7",
		"III_URL":                      "ws://engine:1234/",
		"OTEL_METRICS_ENABLED":         "off",
		"OTEL_SPANS_FLUSH_INTERVAL_MS": "250",
		"OTEL_LOGS_FLUSH_INTERVAL_MS":  "2.5",
		"OTEL_LOGS_BATCH_SIZE":         "10",
	}))
	if err != nil {
		t.Fatal(err)
	}
	if cfg.ServiceName != "orders" || cfg.ServiceVersion != "2.0.0" || cfg.ServiceNamespace != "shop" || cfg.ServiceInstanceID != "inst-7" {
		t.Errorf("service from env: %+v", cfg)
	}
	if cfg.Endpoint != "ws://engine:1234/otel" {
		t.Errorf("endpoint %q", cfg.Endpoint)
	}
	if cfg.MetricsEnabled {
		t.Error("OTEL_METRICS_ENABLED=off did not disable metrics")
	}
	if cfg.SpansFlushInterval != 250*time.Millisecond || cfg.LogsFlushInterval != 2500*time.Microsecond || cfg.LogsBatchSize != 10 {
		t.Errorf("env intervals %v %v batch %d", cfg.SpansFlushInterval, cfg.LogsFlushInterval, cfg.LogsBatchSize)
	}
}

func TestResolveExplicitWinsOverEnvironment(t *testing.T) {
	env := envMap(map[string]string{
		"OTEL_ENABLED": "false", "OTEL_SERVICE_NAME": "env", "III_URL": "ws://env:1",
		"OTEL_METRICS_ENABLED": "false", "OTEL_SPANS_FLUSH_INTERVAL_MS": "999", "OTEL_LOGS_BATCH_SIZE": "9",
	})
	r := Reconnect{InitialDelay: time.Millisecond, MaxDelay: time.Second, BackoffMultiplier: 1, MaxRetries: 3}
	cfg, err := Resolve(Options{
		Enabled: boolPtr(true), ServiceName: "explicit", EngineWSURL: "ws://explicit:2",
		EngineWSHeaders: http.Header{"Authorization": {"Bearer t"}}, MetricsEnabled: boolPtr(true),
		SpansFlushInterval: 5 * time.Millisecond, LogsBatchSize: 4, FetchInstrumentationEnabled: boolPtr(false), Reconnection: &r,
	}, env)
	if err != nil {
		t.Fatal(err)
	}
	if !cfg.Enabled || cfg.ServiceName != "explicit" || cfg.Endpoint != "ws://explicit:2/otel" || !cfg.MetricsEnabled ||
		cfg.SpansFlushInterval != 5*time.Millisecond || cfg.LogsBatchSize != 4 || cfg.FetchInstrumentationEnabled || cfg.Reconnection != r ||
		cfg.EngineWSHeaders.Get("Authorization") != "Bearer t" {
		t.Errorf("explicit options lost: %+v", cfg)
	}
}

// Node parseBoolEnv: only false/0/no/off (any case) disable; set-but-empty enables.
func TestParseBoolEnv(t *testing.T) {
	for _, tc := range []struct {
		value string
		set   bool
		want  bool
	}{
		{set: false, want: true},
		{value: "false", set: true, want: false},
		{value: "FALSE", set: true, want: false},
		{value: "0", set: true, want: false},
		{value: "no", set: true, want: false},
		{value: "Off", set: true, want: false},
		{value: "", set: true, want: true},
		{value: "true", set: true, want: true},
		{value: "yes", set: true, want: true},
		{value: " false", set: true, want: true},
	} {
		m := map[string]string{}
		if tc.set {
			m["OTEL_ENABLED"] = tc.value
		}
		if got := parseBoolEnv(envMap(m), "OTEL_ENABLED", true); got != tc.want {
			t.Errorf("OTEL_ENABLED=%q (set %v) = %v, want %v", tc.value, tc.set, got, tc.want)
		}
	}
}

func TestFlushIntervalAndBatchSizeValidation(t *testing.T) {
	for value, want := range map[string]time.Duration{"-5": 100 * time.Millisecond, "abc": 100 * time.Millisecond, " ": 100 * time.Millisecond, "0": time.Millisecond, "NaN": 100 * time.Millisecond} {
		if got := resolveFlushInterval(0, envMap(map[string]string{"K": value}), "K", 100*time.Millisecond); got != want {
			t.Errorf("flush interval %q = %v, want %v", value, got, want)
		}
	}
	for value, want := range map[string]int{"0": 1, "2.5": 1, "3": 3, "x": 1} {
		cfg, _ := Resolve(Options{}, envMap(map[string]string{"OTEL_LOGS_BATCH_SIZE": value}))
		if cfg.LogsBatchSize != want {
			t.Errorf("OTEL_LOGS_BATCH_SIZE=%q -> %d, want %d", value, cfg.LogsBatchSize, want)
		}
	}
}

// The cases of the Rust SDK's append_otel_path tests, plus a base path.
func TestAppendOtelPath(t *testing.T) {
	for in, want := range map[string]string{
		"ws://localhost:49134":                 "ws://localhost:49134/otel",
		"ws://localhost:49134/":                "ws://localhost:49134/otel",
		"ws://localhost:49134/otel":            "ws://localhost:49134/otel",
		"ws://localhost:49134?token=abc":       "ws://localhost:49134/otel?token=abc",
		"ws://localhost:49134/otel?token=abc":  "ws://localhost:49134/otel?token=abc",
		"ws://localhost:49134#frag":            "ws://localhost:49134/otel#frag",
		"ws://localhost:49134/otel/?token=abc": "ws://localhost:49134/otel?token=abc",
		"wss://engine.example.com/base//":      "wss://engine.example.com/base/otel",
	} {
		got, err := AppendOtelPath(in)
		if err != nil || got != want {
			t.Errorf("AppendOtelPath(%q) = %q, %v; want %q", in, got, err, want)
		}
	}
	if _, err := AppendOtelPath("not a url"); err == nil {
		t.Error("AppendOtelPath accepted a URL without scheme and host")
	}
	// A disabled configuration never parses the URL.
	if _, err := Resolve(Options{Enabled: boolPtr(false), EngineWSURL: "::bad"}, envMap(nil)); err != nil {
		t.Errorf("disabled config failed on its URL: %v", err)
	}
}
