# iii Go observability

OpenTelemetry and logging primitives shared across iii Go SDK workers — the Go
counterpart of the Rust `iii-observability` crate and the Node `@iii-dev/observability`
package.

This is a **separate module** from the [iii Go SDK](../iii): the SDK carries W3C trace
context (`traceparent` / `baggage`) on the wire and ships a no-op OTel seam, while this
package is the real OpenTelemetry wiring a worker opts into.

```bash
go get github.com/iii-hq/iii/sdk/packages/go/observability
```

Requires Go 1.25+ (pulled in by the OpenTelemetry OTLP exporter's dependency graph).

## Logger

A structured logger that emits OpenTelemetry `LogRecord`s correlated with the active
span. Pass structured data as a map (not string interpolation) so your backend can filter
and aggregate on it. When `Init` has not run, it falls back to the standard library
`slog`.

```go
import "github.com/iii-hq/iii/sdk/packages/go/observability"

logger := observability.NewLogger()

logger.Info(ctx, "Worker connected", nil)
logger.Info(ctx, "Order processed", map[string]any{"order_id": "ord_123", "amount": 49.99})
logger.Warn(ctx, "Retry attempt", map[string]any{"attempt": 3, "max_retries": 5})
logger.Error(ctx, "Payment failed", map[string]any{"gateway": "stripe", "code": "card_declined"})
```

Nested objects and arrays are preserved as structured OTLP attributes (`KeyValueList` /
`Slice`), not stringified.

## Init

Wire an OpenTelemetry logger provider once at startup and shut it down before exit:

```go
shutdown, err := observability.Init(ctx, observability.DefaultConfig()) // stdout, service "iii"
if err != nil {
	log.Fatal(err)
}
defer shutdown(ctx)

// Or ship to an OTLP/HTTP collector:
observability.Init(ctx, observability.OtelConfig{
	ServiceName:  "my-worker",
	Exporter:     observability.ExporterOTLP,
	OTLPEndpoint: "http://localhost:4318",
})
```

## Scope

v1 wires **logs** (the `Logger` and its OTLP/stdout export), which is what workers reach
for first. Trace and metric exporters, span operations, baggage propagation, and HTTP
instrumentation — present in the Rust/Node packages — are a planned next layer.

## Resources

- [iii Go SDK](../iii) · [Rust observability](../../rust/observability) — the reference this mirrors
- [iii engine](https://github.com/iii-hq/iii)
