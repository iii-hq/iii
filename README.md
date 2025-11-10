Process Communication Engine
============================

A lightweight Rust playground for experimenting with process-to-process RPC over a single
length-delimited TCP stream. The `server` binary accepts arbitrary “clients” that
register the methods they implement (with JSON Schema validation), and then routes
`invoke`, `notify`, and heartbeat frames between them. The repo also ships with an
example Rust client (`src/client.rs`) and a parity Python client (`python_client.py`)
to demonstrate cross-language interoperability.

Features at a Glance
--------------------
- JSON Schema validation for every advertised method (ensures bad clients are rejected early).
- Concurrent client management via Tokio + `DashMap`, with automatic cleanup on disconnect.
- Built-in heartbeat/notify loop so the engine can detect stalled peers.
- Optional runtime limit for clients—helpful for integration tests and CI jobs.
- Message protocol expressed as plain Serde enums, which keeps the on-wire format obvious.

Repository Layout
-----------------
- `src/server.rs` – TCP router that tracks connected clients, validates registrations, and forwards messages.
- `src/client.rs` – Async Rust client showcasing method registration, heartbeat emission, and request handling.
- `src/protocol.rs` – Shared message and schema definitions used by both binaries.
- `python_client.py` – asyncio-based client with the same framing logic (useful for scripting or quick demos).
- `Makefile` – Convenience targets for `run_socket_server` and `run_socket_client`.
- `requirements.txt` – Python dependencies if you want to experiment with the (WIP) gRPC surface.

Prerequisites
-------------
- Rust 1.80+ (Rust 2024 edition used by the crate; install via `rustup`).
- Python 3.10+ (only if you plan to run `python_client.py`).
- `make` (optional, for the convenience targets).

Running the Server
------------------
```bash
# from /engine
cargo run --bin server
# or
make run_socket_server
```

The server listens on `127.0.0.1:8080` by default. Adjust the address by editing
`src/server.rs` or by wrapping the listener in your own launcher binary.

Running the Rust Example Client
-------------------------------
```bash
cargo run --bin client -- --addr 127.0.0.1:8080 --heartbeat 5 --run-for 30
# environment variable equivalents
ENGINE_ADDR=127.0.0.1:8080 ENGINE_HEARTBEAT=5 ENGINE_RUN_FOR=30 cargo run --bin client
```

Flags & env vars:
- `--addr` / `ENGINE_ADDR`: target engine address (default `127.0.0.1:8080`).
- `--heartbeat` / `ENGINE_HEARTBEAT`: seconds between heartbeat `notify` frames (set to 0 to disable).
- `--run-for` / `ENGINE_RUN_FOR`: optional runtime limit in seconds; omit to run indefinitely.

Using the Python Client
-----------------------
```bash
python3 -m venv .venv && source .venv/bin/activate
pip install -r requirements.txt
python python_client.py --addr 127.0.0.1:8080 --heartbeat 5 --run-for 30
```

`python_client.py` mirrors the Rust client: it registers `math.add`, listens for
`call` messages, emits heartbeats, and prints `notify/result` frames as they arrive.
This is a convenient way to script workloads or reproduce issues without recompiling Rust.

Message Protocol Cheat Sheet
----------------------------
All frames are JSON serialized and prefixed with a 4-byte big-endian length (Tokio’s
`LengthDelimitedCodec` format). The primary message types are defined in `src/protocol.rs`:

- `register`: `{ "type": "register", "name": "...", "methods": [MethodDef] }`
- `invoke`: `{ "type": "invokefunctionmessage", "id": "...", "to": "...", "method": "...", "params": {...} }`
- `result`: `{ "type": "result", "id": "...", "ok": true, "result": {...} }`
- `notify`: fire-and-forget events or heartbeats; `to` may be omitted to broadcast.
- `ping` / `pong`: keepalive frames used by the sample clients.
- `error`: structured errors when schema compilation or invocation fails.

Example register payload:

```json
{
  "type": "register",
  "name": "example_client",
  "description": "Provides math.add",
  "methods": [
    {
      "name": "math.add",
      "params_schema": {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "properties": {
          "a": { "type": "number" },
          "b": { "type": "number" }
        },
        "required": ["a", "b"],
        "additionalProperties": false
      },
      "result_schema": { "type": "number" }
    }
  ]
}
```

Development Notes
-----------------
- Lint/format: `cargo fmt && cargo clippy -- -D warnings`.
- Integration smoke test: run the server, then start one or more clients (Rust or Python) and observe routed `InvokeFunctionMessage` logs.
- The `routing` map inside `Engine` currently keys by `method + client_id`. Extend this if you need round-robin or fan-out semantics.
- `requirements.txt` currently only lists packages used for a future gRPC transport; the pure TCP client requires no third-party modules.

Next Ideas
----------
- Add persistence or service discovery so clients can restart without re-registering.
- Gate schema compilation failures behind a retry or quarantine flow.
- Extend the Makefile with `fmt`, `clippy`, and `test` helpers for easier CI wiring.
