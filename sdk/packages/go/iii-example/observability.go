package main

import (
	"context"
	"log"
	"os"
	"time"

	obs "github.com/iii-hq/iii/sdk/packages/go/observability"
)

// demoObservability wires the iii Go observability package and emits structured logs at
// every level. With the bundled docker-compose stack running, the logs flow over OTLP to
// the collector and into the OpenObserve UI; without it, records print to stdout.
//
//	docker compose up -d
//	OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318 go run .
//	open http://localhost:5080   # root@example.com / Complexpass#123
//
// It returns a shutdown func that main defers so the batch processor flushes before exit.
func demoObservability(ctx context.Context) func() {
	cfg := obs.DefaultConfig() // stdout, service "iii"
	cfg.ServiceName = "go-observability-example"
	if endpoint := os.Getenv("OTEL_EXPORTER_OTLP_ENDPOINT"); endpoint != "" {
		cfg.Exporter = obs.ExporterOTLP
		cfg.OTLPEndpoint = endpoint
		log.Printf("observability: exporting OTLP to %s", endpoint)
	} else {
		log.Print("observability: no OTEL_EXPORTER_OTLP_ENDPOINT set; OTel records print to stdout")
	}

	shutdown, err := obs.Init(ctx, cfg)
	if err != nil {
		log.Printf("observability: init failed: %v", err)
		return func() {}
	}

	logger := obs.NewLogger()
	logger.Info(ctx, "Worker connected", nil)
	// Native Go numerics and nested maps/slices are preserved as structured OTLP
	// attributes (not stringified).
	logger.Info(ctx, "Order processed", map[string]any{
		"order_id": "ord_123",
		"amount":   49.99,
		"currency": "USD",
		"items":    []any{"sku-1", "sku-2"},
		"customer": map[string]any{"id": 42, "vip": true},
	})
	logger.Warn(ctx, "Retry attempt", map[string]any{"attempt": 3, "max_retries": 5})
	logger.Error(ctx, "Payment failed", map[string]any{"gateway": "stripe", "code": "card_declined"})

	return func() {
		time.Sleep(500 * time.Millisecond) // let the batch processor flush
		_ = shutdown(ctx)
	}
}
