# HTTP request-body retention probe

This is a standalone reproduction: an engine, the external Rust `http` worker,
Node worker and rate-controlled GET/POST client. No application services, data or
credentials are needed. `POST /probe` returns buffered JSON without opening
`request_body`; `GET /probe` is the empty-body control.

```sh
cd engine/tests/fixtures/post-retention
npm install --ignore-scripts
cd ../../../..
python3 engine/tests/fixtures/post-retention/run.py \
  --engine /absolute/path/to/iii \
  --http /absolute/path/to/http \
  --node /absolute/path/to/node \
  --out target/post-retention/valid-before-2-P \
  --mode P --seconds 360 --arena 2 --idle 365
```

The runner refuses occupied ports (3711 HTTP, 49734 engine) and needs >50 GiB
free. It starts fresh processes and configuration per scenario, with an explicitly
constructed environment (no inherited worker identity). It seeds the authoritative
`iii-observability` registry file to disable traces, metrics and stored logs:
legacy `OTEL_ENABLED=false` alone does **not** override current engine defaults.
It stops only its own child PIDs in `finally`.

Repeat with `--mode G` and `--arena unset`, before and after the fix. `--idle 365`
continues sampling after the load to observe the original 300–360-second channel
expiration. RSS, anonymous RSS, fd/thread counts and host load are sampled every
second. `check.log` verifies raw request bodies (100 B, 4 KB and 1 MiB, SHA-256),
SSE headers/five events, and a generic channel with a late reader after writer EOF.
Each 360-second load should report 7,200 successful requests and zero errors.

```sh
python3 engine/tests/fixtures/post-retention/analyze.py target/post-retention
```

The reported slope is the median of pairwise slopes of ten-second median RSS
bins in seconds 30–270, divided by 20 requests/second. This avoids startup warmup
and the first TTL sweep; raw CSVs also show the six-minute endpoint and idle tail.
Do not accept a run that misses the configured rate or has errors.

The deterministic regression is in `workers::worker::ws_handler::tests`:

```sh
cargo test -p iii --lib queued_binary_does_not_retain_websocket_read_buffer
```

Before the fix a queued 100-byte binary frame uniquely owns a 131,066-byte
allocation after its WebSocket closes; after the fix it owns only its payload.
Channel lifetimes, ordering, bounded queue size and late-reader behavior remain
unchanged. The tradeoff is one payload-sized allocation/copy per binary frame.
