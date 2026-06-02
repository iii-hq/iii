# iii Go SDK — examples

Runnable examples for the [iii Go SDK](../iii). This is a separate Go module that depends
on the sibling `iii` and [`observability`](../observability) modules via `replace`
directives, mirroring the Rust `iii-example` crate and the Node `iii-example` package.

Each file demonstrates one feature; `main.go` wires them together, connects, runs a few
demo invocations, then serves so the triggers stay live.

| File | Demonstrates |
|---|---|
| `http.go` | An HTTP-triggered function (`hello::greet` at `POST /greet`), using the engine's HTTP request/response envelope. |
| `cron.go` | A function bound to the engine's built-in `cron` trigger type. |
| `triggertype.go` | A **custom trigger type** (`interval`) this worker implements via `TriggerHandler` — the engine calls the worker to start/stop trigger instances. |
| `logger.go` | Writing to the engine log at each level via the `engine::log::*` functions. |
| `channels.go` | A streaming data channel round trip (write bytes + a message, read them back). |
| `observability.go` | Structured OpenTelemetry logging via the [`observability`](../observability) package, with the bundled telemetry stack below. |

## Run

```bash
# 1. Start an engine (in another terminal)
iii --use-default-config            # engine on ws://localhost:49134, HTTP on :3111

# 2. Run the worker
go run .                            # from sdk/packages/go/iii-example

# 3. Call the HTTP trigger
curl -X POST localhost:3111/greet -d '{"name":"world"}'
# => {"message":"Hello, world!"}
```

Set `III_URL` to point at a non-default engine endpoint.

## Telemetry stack (observability example)

`docker-compose.yaml` + `otel-collector-config.yaml` bring up an OpenTelemetry Collector
and [OpenObserve](https://openobserve.ai) so the `observability.go` logs are visible in a
UI. Mirrors the Node/Python `iii-example` telemetry stacks.

```bash
docker compose up -d                                    # collector (:4318) + OpenObserve (:5080)
OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318 go run .
open http://localhost:5080                              # root@example.com / Complexpass#123
docker compose logs -f otel-collector                   # or watch records on the collector
```

Without `OTEL_EXPORTER_OTLP_ENDPOINT`, the observability example prints OTel records to
stdout — no stack required.