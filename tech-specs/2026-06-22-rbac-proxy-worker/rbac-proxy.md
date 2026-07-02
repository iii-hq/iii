# rbac-proxy

Worker prefix: `rbac-proxy::*` · Deploy mode: `binary` (Rust) · Config id:
`rbac-proxy`

## Definition

`rbac-proxy` is a binary Rust worker that opens a public WebSocket port, speaks
the iii worker protocol, and reverse-proxies every connection — functions and
channels — to a trusted engine listener while enforcing RBAC at the boundary. It
is structurally the [`console`](../../console) worker with the RBAC interceptor
of [protocol-interception.md](protocol-interception.md) spliced into the proxy
pump and a second proxied route for `/ws/channels/{id}`.

It is a **pure boundary**: it stores nothing, runs no agent logic, and adds no
business functions. It registers exactly one public function for health probes
(`rbac-proxy::status`) and one internal config-reload handler
(`rbac-proxy::on-config-change`).

## Connection planes

The worker holds two distinct kinds of engine connection (see
[README § Architecture](README.md#architecture)):

### Control connection (one, persistent)

The proxy's own worker identity, created at boot with `register_worker(...)`
like every binary worker. Used for:

- **`configuration`-worker integration** — register the `rbac-proxy` schema,
  fetch the authoritative config, bind the change trigger.
- **Invoking the operator's functions** — the `auth_function_id`,
  `middleware_function_id`, and the three registration hooks are normal engine
  functions; the proxy calls them with `iii.trigger(...)`. This is the direct
  analogue of how the engine's worker-manager invokes them with
  `engine.call(...)`.
- **The catalog cache feed** — `engine::functions::list` reads and the
  `engine::functions-available` subscription
  ([engine-overrides.md § Catalog & binding caches](engine-overrides.md#catalog--binding-caches)).
- **Registering `rbac-proxy::status`.**

### Data connections (one per downstream worker)

Exactly the `console` model: each inbound connection on the public port gets a
fresh outbound WebSocket to `engine_url`, with the two halves pumped through the
interceptor. Per-connection `worker_id`, per-connection registrations, and
cleanup-on-disconnect are inherited from the engine's existing per-connection
semantics — when the downstream socket closes, the proxy closes the upstream,
and the engine tears down that connection's functions/triggers.

### Resilience

The control connection is a single point of dependency (auth, middleware, hooks,
catalog/binding caches all run over it), so its failure mode is specified:

- **Control connection drops** → the SDK reconnects with backoff (the standard
  worker SDK behaviour `console` already relies on). While it is down, the proxy
  **fails closed**: new upgrades that need the auth function are rejected with the
  error frame, and gated invocations needing middleware/hooks are denied — a
  broken control plane never opens the door (same contract as
  [rbac.md § Fail closed](rbac.md#authentication)).
- **Established data connections are independent** — each is its own upstream
  WebSocket, so a control-connection blip does **not** tear them down; in-flight
  pumping continues. Only operations that call back through the control plane
  (auth on a *new* upgrade, middleware/hook on a *new* gated call) are affected.
- **A data connection's upstream drops** → the proxy closes the matching
  downstream socket; the downstream worker's SDK reconnects and re-authenticates,
  getting a fresh session. The proxy does not silently re-home a live downstream
  connection onto a new upstream.

## File layout

Mirrors `console` (see [`binary-worker.md`](../../docs/sops/binary-worker.md) for
the canonical layout); new/changed modules called out:

```text
rbac-proxy/
├── iii.worker.yaml          # name: rbac-proxy, deploy: binary, dependencies: { configuration }
├── Cargo.toml               # [workspace] + [[bin]] + [lib]; iii-sdk pinned; axum/tokio/tungstenite
├── build.rs                 # exposes TARGET (no web assets to embed)
├── src/
│   ├── main.rs              # boot order (below); SIGINT+SIGTERM graceful shutdown
│   ├── lib.rs               # pub mod { config, configuration, server, proxy, interceptor, rbac, engine_overrides, channels, functions, manifest }
│   ├── config.rs            # WorkerConfig (+ JsonSchema, from_json/to_json/json_schema/boot_signature)
│   ├── configuration.rs     # ConfigCell, register/fetch, on-config-change, port rebind
│   ├── server.rs            # axum router: GET / (worker proto) + GET /ws/channels/{id}
│   ├── proxy.rs             # console-style WS pump (data connections)
│   ├── interceptor.rs       # per-frame parse/gate/rewrite (protocol-interception.md)
│   ├── rbac.rs              # vendored is_function_allowed + wildcard + carve-out + ProxySession
│   ├── engine_overrides.rs  # the eight discovery-result rewrites (engine-overrides.md)
│   ├── channels.rs          # /ws/channels/{id} bridge
│   ├── manifest.rs          # build_manifest() for the registry
│   └── functions/
│       ├── mod.rs           # register_all → status
│       └── status.rs        # rbac-proxy::status (typed)
└── tests/                   # manifest, schemas (golden), interception unit tests, integration
```

`rbac.rs` and `engine_overrides.rs` vendor the engine's decision logic
(`engine/src/workers/worker/rbac_config.rs`) verbatim so the proxy and a
`worker-gateway` listener never diverge. A unit test asserts the vendored
matcher matches the engine's against a shared fixture table.

## Boot order

The proxy is a Tier-1 `configuration` integration with one structural resource
(the listener), so it follows the SOP boot order
([`configuration.md` §4c](../../docs/sops/configuration.md)):

```text
1. parse CLI                       (--url engine seed, --config optional one-time seed, --manifest)
2. register_worker(--url)          # control connection
3. register_config(seed) + fetch_config()        # REQUIRED boot dependency (fatal on failure)
4. build ProxyState from the fetched config; bind the public listener on host:port
5. register rbac-proxy::status; start the catalog-cache feed
6. spawn the axum server (worker-proto route + /ws/channels bridge) on the bound listener
7. register_config_trigger          # LAST — so the handler closes over fully-built state
8. wait_for_shutdown_signal (SIGINT + SIGTERM) → drain → iii.shutdown_async()
```

`engine_url` for the data plane defaults to the same `--url` the control
connection used; a config override lets the proxy front a different (e.g.
remote) engine.

## Configuration

Config lives in the `configuration` worker under id `rbac-proxy`. No
`config.yaml` is committed; `WorkerConfig::default()` seeds first boot
([`configuration.md` §4d](../../docs/sops/configuration.md)).

```rust
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkerConfig {
    #[serde(default = "default_host")]            pub host: String,            // "0.0.0.0"
    #[serde(default = "default_port")]            pub port: u16,               // the public RBAC port (req #2)
    #[serde(default = "default_engine_url")]      pub engine_url: String,      // ws://127.0.0.1:49134
    #[serde(default)]                             pub middleware_function_id: Option<String>,
    #[serde(default)]                             pub expose_worker_internals: bool, // false
    #[serde(default)]                             pub rbac: RbacConfig,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct RbacConfig {
    #[serde(default)] pub auth_function_id: Option<String>,
    #[serde(default)] pub expose_functions: Vec<FunctionFilter>,   // match("...") | { metadata: {...} }
    #[serde(default)] pub on_function_registration_function_id: Option<String>,
    #[serde(default)] pub on_trigger_registration_function_id: Option<String>,
    #[serde(default)] pub on_trigger_type_registration_function_id: Option<String>,
}
```

This is intentionally the same field set as the engine's `WorkerManagerConfig` /
the devexp `gateway:` block, so an operator's mental model transfers and a
migration to/from `worker-gateway` is a config copy. `FunctionFilter` deserializes
the `match("pattern")` and `metadata:` forms exactly as the engine's
`rbac_config.rs` does.

### Hot reload

The proxy uses the Tier-1 `ConfigCell + targeted rebuild` pattern
([`configuration.md` §6](../../docs/sops/configuration.md)), matching
`context-manager`/`approval-gate`. The reload tier split:

| Field(s) | Class | On change |
|---|---|---|
| `host`, `port` | **structural** | rebind the public listener: bind the new `host:port`, swap the live listener, drop the old; **bind failure keeps the previous listener and config (last-good)** |
| `engine_url` | **structural** | new data connections dial the new engine; existing connections finish on the old upstream (no forced cutover) |
| `rbac.*`, `middleware_function_id`, `expose_worker_internals` | **tuning** | read from the live `ConfigCell` snapshot per connection/per call — next connection picks up new auth/expose/hook/middleware/leak settings; in-flight connections keep the session derived at their upgrade |

`boot_signature()` returns `{ host, port, engine_url }`; the
`rbac-proxy::on-config-change` handler compares signatures to decide rebind vs
snapshot-swap. It is a **typed** handler (`OnConfigChangeEvent` → `{ ok }`) that
**re-fetches** via `configuration::get` and ignores the trigger payload — never a
`serde_json::Value` handler. It is registered in `configuration.rs` (off the
public `catalog()`).

```mermaid
sequenceDiagram
  participant CFG as configuration worker
  participant H as rbac-proxy::on-config-change
  participant S as ProxyState
  CFG->>H: configuration:updated (id=rbac-proxy)
  H->>CFG: configuration::get(rbac-proxy)   %% re-fetch, ignore payload
  alt host/port/engine_url changed (signature)
    H->>S: bind new listener → swap → drop old (last-good on failure)
  else tuning-only
    H->>S: swap ConfigCell snapshot
  end
```

### Permissions

Add the defense-in-depth deny next to the existing integrated-worker entries in
`iii-permissions.yaml` ([`configuration.md` §4e](../../docs/sops/configuration.md)):

```yaml
rbac-proxy:
  - '!rbac-proxy::on-config-change'   # internal reload handler — never agent-callable
```

(No `config-status` surface in the Tier-1 pattern, so nothing else to deny.)

## Functions

The proxy registers one public function. The two-step `configuration` trigger
handler (`rbac-proxy::on-config-change`) is internal and not part of the public
catalog.

### `rbac-proxy::status`

Health/identity probe for `iii worker info` smoke tests. Invocation: **sync**.

```rust
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct StatusInput {}

#[derive(Debug, Serialize, JsonSchema)]
pub struct StatusOutput {
    pub host: String,            // bound public host
    pub port: u16,               // bound public RBAC port
    pub engine_url: String,      // upstream engine (redacted of credentials)
    pub rbac_enabled: bool,      // auth_function_id is set
    pub active_connections: u32, // live downstream connections
    pub version: String,         // Cargo.toml version
}
```

## Agent exposure

Deny-by-default for in-run agents (see
[2026-06-08-agentic README § Security model](../2026-06-08-agentic/README.md#security-model)).

- **Deny:** `rbac-proxy::on-config-change` (internal reload; permissions deny
  above).
- **Allow with care:** `rbac-proxy::status` — read-only operational metadata. It
  reports the engine URL and bound port; deny it on deployments where that
  topology is itself sensitive. The handler **must** pass `engine_url` through a
  `console`-style `redact_url` (strip any `user:pass@` userinfo) before
  returning, so a credentialed `wss://user:secret@host` upstream never leaks to
  an agent-callable probe.

The proxy is an *enforcement* surface, not a callable API; its security value is
the boundary it draws, configured by the operator, never by an agent.

## Boundaries

- Does **not** modify the engine, the protocol, or any engine port. It is pure
  worker code against the existing `Message` protocol and the `engine::*` request/
  response shapes.
- Does **not** persist state or run agent logic. The only state it holds is
  per-connection `ProxySession`s, a short-TTL function-catalog cache, and the
  pending-override map — all transient.
- Does **not** replace the engine's RBAC; it is an alternative *home* for it.
  An engine can run `worker-gateway`, `rbac-proxy`, or both
  ([README § coexistence](README.md#why-this-exists-and-how-it-relates-to-the-engines-worker-gateway)).
- Does **not** terminate or re-issue channel `access_key`s — channel sockets are
  relayed and the engine validates the capability token.
- Does **not** itself authenticate the channel data socket beyond relaying the
  `access_key`; per-session channel binding is an optional bridge extension
  ([protocol-interception.md § Channel bridge](protocol-interception.md#channel-bridge)).

## Dependencies

- **`configuration` worker** — required boot dependency (schema register +
  fetch). Declared in `iii.worker.yaml` `dependencies`.
- **A trusted engine listener** at `engine_url` with **no** `rbac` block (the
  proxy is the boundary; the engine port is internal-only).
- **Operator-registered functions** (optional): the `auth_function_id`,
  `middleware_function_id`, and registration-hook functions, registered against
  the engine from any SDK.

```yaml title="iii.worker.yaml"
iii: v1
name: rbac-proxy
language: rust
deploy: binary
manifest: Cargo.toml
bin: rbac-proxy
description: RBAC boundary proxy for the iii worker protocol — auth, gating, namespacing, middleware, and engine:: result filtering on its own port.

dependencies:
  configuration: "^0.19.0"
```

## Testing

Engine-free unit tests plus an opt-in integration test, per
[`binary-worker.md` §9](../../docs/sops/binary-worker.md):

- **`tests/manifest.rs`** — `--manifest` round-trips and validates.
- **`tests/schemas.rs`** — golden catalog test pinning `rbac-proxy::status`'s
  typed request/response schema; asserts no `AnyValue` schemas.
- **Interception unit tests** (`interceptor.rs` / `rbac.rs`):
  - access-resolution table (forbidden > allowed > carve-out > expose > deny),
    shared fixture matched against the vendored engine matcher;
  - deny synthesizes a `FORBIDDEN` `InvocationResult` with the echoed
    `invocation_id`;
  - prefix resolution: own bare `foo` → `tenant1::foo`; foreign
    `api::users::list` matched against `expose_functions` unprefixed; dispatch
    strips the prefix back to `foo`;
  - trigger registration: binding to an exposed foreign function succeeds;
    binding to a forbidden / non-exposed foreign function returns
    `TriggerRegistrationResult{ error: REGISTRATION_DENIED }`; binding to the
    session's own registered function succeeds even when the prefixed id is not
    in `expose_functions`;
  - the auth-rejection wire shape: `{"type":"error",...}` then Close, no
    `WorkerRegistered`, no upstream opened.
- **Engine-override tests** (`engine_overrides.rs`): for each of the eight
  discovery functions, a fixture engine response in → filtered response out;
  the catalog-cache-driven `workers::list` filter and the registered-triggers
  binding-index join for `triggers::info` `instance_count`; the worker-internals
  strip (per the `WorkerSummary` vs `WorkerDetailEnvelope` field split); the
  prefix strip in results; and the denied-vs-missing error code (`FORBIDDEN`
  default, optional `NOT_FOUND` hardening).
- **`tests/integration.rs`** (self-skips when the `iii` binary is absent):
  boot an engine + the proxy, connect an SDK worker through the proxy port with
  a bearer token, assert (a) an exposed call succeeds, (b) a forbidden call
  rejects with `FORBIDDEN`, (c) a trigger bound to a forbidden function is
  rejected at registration, (d) `createChannel()` round-trips a payload through
  `/ws/channels/{id}`, and (e) `engine::functions::list` returns only exposed
  ids.
- **Config tests**: `boot_signature` tuning-vs-structural; JSON round-trip; the
  port-rebind path swaps the listener and a bind failure keeps last-good.
