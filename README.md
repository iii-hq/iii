# Engine

This workspace hosts a Rust gRPC routing engine plus a pair of sample Python workers and example
clients in Node.js and Lua. The engine accepts incoming `Process`/`StreamProcess` requests, forwards
them to registered workers, and relays the responses back to the caller.

## Project Layout
- `src/main.rs` – engine implementation that registers workers and proxies requests.
- `proto/engine.proto` – shared protobuf definitions used by the engine, workers, and clients.
- `worker_py/` – Python worker examples (`server.py` for unary calls, `stream_server.py` for streaming).
- `client.js` – Node.js sample client that exercises the engine API.
- `client_lua/grpcurl_client.lua` – Lua sample client built on top of `grpcurl`.
- `Makefile` – convenience targets for Python setup and code generation.

## Prerequisites
- Rust toolchain with edition 2024 support (install via [`rustup`](https://rustup.rs); `rustup default
  nightly` works if your stable toolchain predates the 2024 edition).
- `protoc` (Protocol Buffers compiler) available on PATH – required by `tonic-build` and Python stub generation.
- Python 3.10+ with `venv` module.
- Node.js 18+ and `npm`.
- Optional clients:
  - `lua` 5.4+, `luarocks`, and the `dkjson` rock.
  - [`grpcurl`](https://github.com/fullstorydev/grpcurl).

## Setup

### 1. Rust engine
```bash
# from engine/
rustup toolchain install nightly
rustup default nightly   # only required if your stable toolchain cannot build edition 2024
cargo build
```

### 2. Python workers
```bash
make setup_python          # creates .venv/ and installs grpcio / grpcio-tools
make generate_python       # regenerates worker stubs from proto/ (run when proto changes)
```

If you prefer manual steps:
```bash
python -m venv .venv
source .venv/bin/activate
pip install -r requirements.txt
python -m grpc_tools.protoc -I proto --python_out=worker_py --grpc_python_out=worker_py proto/engine.proto
```

### 3. Node.js client
```bash
npm install
```

### 4. Lua + grpcurl client (optional)
```bash
brew install grpcurl        # or your platform’s package manager
luarocks install dkjson
```

## Running the Demo
Open three terminals inside `engine/`:

1. **Engine** – start the Rust server (defaults to `0.0.0.0:50051`):
   ```bash
   cargo run
   ```
   - Override listening address with `ENGINE_ADDR=127.0.0.1:50051 cargo run`.
   - Pre-register a default worker by pointing `WORKER_ADDR` at a reachable worker endpoint.

2. **Unary worker** – register the formatting service (defaults to `text-formatter` on `http://127.0.0.1:50052`):
   ```bash
   source .venv/bin/activate
   ENGINE_ADDR=localhost:50051 SERVICE_NAME=text-formatter SERVICE_ADDR=http://127.0.0.1:50052 \
     python worker_py/server.py
   ```
   The script connects to the engine, registers its methods, and waits for requests.

3. **Streaming worker** – register the streaming service (`text-streamer` on `http://127.0.0.1:50053`):
   ```bash
   source .venv/bin/activate
   ENGINE_ADDR=localhost:50051 SERVICE_NAME=text-streamer SERVICE_ADDR=http://127.0.0.1:50053 \
     python worker_py/stream_server.py
   ```

With the services running you can drive them from the sample clients:
- **Node.js:** `ENGINE_ADDR=localhost:50051 node client.js`
- **Lua/grpcurl:** `ENGINE_ADDR=localhost:50051 lua client_lua/grpcurl_client.lua proto/engine.proto`

Both clients will list the registered services, issue a unary `Process` call, and exercise the streaming API.

## Regenerating Code After Proto Changes
- Rust: `cargo clean` (optional) then `cargo build`; `tonic-build` compiles the protobuf during the build.
- Python: rerun `make generate_python` (or the equivalent `python -m grpc_tools.protoc` command).
- Node & Lua clients read `proto/engine.proto` directly; restart them after updating the proto.

## Troubleshooting
- `tonic-build` errors about `protoc`: ensure the compiler is installed and visible via `which protoc`.
- Python worker fails to register: confirm `ENGINE_ADDR` points at a reachable engine instance and that the engine is already listening.
- Makefile target `run_worker_server` expects `worker_py/worker_server.py`; the current unary worker entry point is `worker_py/server.py`. Run the script directly or adjust the target.
- Streaming requests time out: increase the engine timeout by editing `request_timeout` in `src/main.rs` or reduce the worker’s `delay_ms` meta value.

Happy hacking!
