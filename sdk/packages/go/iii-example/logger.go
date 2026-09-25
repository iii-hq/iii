package main

import (
	"context"
	"encoding/json"
	"log"

	iii "github.com/iii-hq/iii/sdk/packages/go/iii"
)

// setupLogger registers a function that writes to the engine log at every level using
// the SDK's log-function constants. Mirrors logger_example.rs — but where the Rust
// example uses the iii-observability Logger, the Go SDK v1 logs via the engine's built-in
// log functions (engine::log::{info,warn,error,debug}) invoked fire-and-forget. A
// dedicated observability package (real OTel logger) is a planned follow-up
// (iii-hq/iii#1719).
func setupLogger(client *iii.Client) {
	if err := client.RegisterFunction("example::logger_demo", func(ctx context.Context, data json.RawMessage) (any, error) {
		logTo := func(fn, message string, fields map[string]any) {
			payload, _ := json.Marshal(map[string]any{"message": message, "data": fields})
			// Fire-and-forget: logging shouldn't block or fail the handler.
			_, _ = client.Trigger(ctx, iii.TriggerRequest{
				FunctionID: fn,
				Data:       payload,
				Action:     iii.VoidAction(),
			})
		}

		logTo(iii.LogInfo, "Processing request", map[string]any{"input": json.RawMessage(data)})
		logTo(iii.LogDebug, "Validating input fields", map[string]any{"step": "validation"})
		logTo(iii.LogWarn, "Using default timeout", map[string]any{"timeout_ms": 5000, "reason": "not configured"})
		logTo(iii.LogInfo, "Request processed successfully", nil)

		return map[string]any{"status": "ok"}, nil
	}); err != nil {
		log.Fatalf("register example::logger_demo: %v", err)
	}
}
