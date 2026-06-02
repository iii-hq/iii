package observability

import (
	"context"
	"encoding/json"
	"testing"

	otellog "go.opentelemetry.io/otel/log"
)

// TestJSONToLogValue checks the JSON→OTel value conversion preserves types and nesting
// (the structured-attributes contract the Rust/Node loggers also uphold).
func TestJSONToLogValue(t *testing.T) {
	tests := []struct {
		name string
		in   string
		kind otellog.Kind
	}{
		{"string", `"hi"`, otellog.KindString},
		{"bool", `true`, otellog.KindBool},
		{"integer stays int", `42`, otellog.KindInt64},
		{"float stays float", `3.5`, otellog.KindFloat64},
		{"array", `[1,2,3]`, otellog.KindSlice},
		{"object", `{"a":1}`, otellog.KindMap},
		{"null becomes empty string", `null`, otellog.KindString},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			var v any
			if err := json.Unmarshal([]byte(tt.in), &v); err != nil {
				t.Fatalf("unmarshal: %v", err)
			}
			got := jsonToLogValue(v)
			if got.Kind() != tt.kind {
				t.Errorf("kind = %v, want %v", got.Kind(), tt.kind)
			}
		})
	}
}

// TestJSONToLogValueNestedObject verifies a nested object round-trips into a Map value
// with the expected keys, rather than being stringified.
func TestJSONToLogValueNestedObject(t *testing.T) {
	var v any
	_ = json.Unmarshal([]byte(`{"order":{"id":"ord_1","amount":49.99},"items":[1,2]}`), &v)
	val := jsonToLogValue(v)
	if val.Kind() != otellog.KindMap {
		t.Fatalf("top-level kind = %v, want Map", val.Kind())
	}
	var sawOrder, sawItems bool
	for _, kv := range val.AsMap() {
		switch kv.Key {
		case "order":
			sawOrder = kv.Value.Kind() == otellog.KindMap
		case "items":
			sawItems = kv.Value.Kind() == otellog.KindSlice
		}
	}
	if !sawOrder {
		t.Error("nested object 'order' should be a Map value")
	}
	if !sawItems {
		t.Error("nested array 'items' should be a Slice value")
	}
}

// TestLoggerFallbackWhenUninitialized confirms Logger does not panic and runs the slog
// fallback path when Init has not installed a provider. (No provider => emitOTel returns
// false => slog.) We only assert it doesn't panic and reports no provider.
func TestLoggerFallbackWhenUninitialized(t *testing.T) {
	if getLoggerProvider() != nil {
		t.Skip("a provider is installed; fallback path not exercised")
	}
	l := NewLogger()
	// Should run the slog fallback without panicking.
	l.Info(context.Background(), "no provider", map[string]any{"k": "v"})
	l.Error(context.Background(), "still fine", nil)
}

// TestAllLevelsEmitViaProvider runs every level through an installed provider so the
// Warn/Debug paths (identical to Info/Error) are exercised, plus DefaultConfig.
func TestAllLevelsEmitViaProvider(t *testing.T) {
	ctx := context.Background()
	cfg := DefaultConfig()
	if cfg.ServiceName != "iii" || cfg.Exporter != ExporterStdout {
		t.Fatalf("DefaultConfig = %+v, want stdout/iii", cfg)
	}
	shutdown, err := Init(ctx, cfg)
	if err != nil {
		t.Fatalf("Init: %v", err)
	}
	defer shutdown(ctx)

	l := NewLogger()
	l.Info(ctx, "i", map[string]any{"a": 1})
	l.Warn(ctx, "w", map[string]any{"b": true})
	l.Error(ctx, "e", nil)
	l.Debug(ctx, "d", map[string]any{"c": "x"})
}

// TestJSONNumberPath covers the json.Number branch (used when a decoder is configured
// with UseNumber), which the plain interface{} decode path does not hit.
func TestJSONNumberPath(t *testing.T) {
	if v := jsonToLogValue(json.Number("7")); v.Kind() != otellog.KindInt64 {
		t.Errorf("json.Number int kind = %v, want Int64", v.Kind())
	}
	if v := jsonToLogValue(json.Number("7.5")); v.Kind() != otellog.KindFloat64 {
		t.Errorf("json.Number float kind = %v, want Float64", v.Kind())
	}
}

// TestInitInstallsAndShutsDownProvider verifies Init installs a provider (so Logger
// emits via OTel) and the returned shutdown removes it.
func TestInitInstallsAndShutsDownProvider(t *testing.T) {
	ctx := context.Background()
	shutdown, err := Init(ctx, OtelConfig{ServiceName: "iii-test", Exporter: ExporterStdout})
	if err != nil {
		t.Fatalf("Init: %v", err)
	}
	if getLoggerProvider() == nil {
		t.Fatal("Init did not install a logger provider")
	}
	// Emitting through the installed provider must take the OTel path (no panic).
	NewLogger().Info(ctx, "hello", map[string]any{"n": 1})

	if err := shutdown(ctx); err != nil {
		t.Errorf("shutdown: %v", err)
	}
	if getLoggerProvider() != nil {
		t.Error("provider should be cleared after shutdown")
	}
}
