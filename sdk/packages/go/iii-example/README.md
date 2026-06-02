# iii Go SDK — examples

Runnable examples for the [iii Go SDK](../iii). This is a separate Go module that depends
on the sibling `iii` module via a `replace` directive (`../iii`), mirroring the Rust
`iii-example` crate and the Node `iii-example` package.

Each file demonstrates one feature with a `setup(client)` function; `main.go` wires them
together, connects, runs a couple of demo invocations, then serves so the triggers stay
live.

| File | Demonstrates |
|---|---|
| `http.go` | An HTTP-triggered function (`hello::greet` at `POST /greet`), using the engine's HTTP request/response envelope. |
| `cron.go` | A function bound to the engine's built-in `cron` trigger type. |
| `triggertype.go` | A **custom trigger type** (`interval`) this worker implements via `TriggerHandler` — the engine calls the worker to start/stop trigger instances. |
| `logger.go` | Writing to the engine log at each level via the `engine::log::*` functions. |
| `channels.go` | A streaming data channel round trip (write bytes + a message, read them back). |

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