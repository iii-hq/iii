# Harness integration and Console E2E tests

The integration suite is a deterministic conformance test for the harness
public turn path. It verifies queue delivery, model streaming, durable
transcripts, native function dispatch, lifecycle delivery, traces, process
supervision, and reporting without a provider connection or model inference.

## Definition

Each scenario starts a fresh isolated iii stack with the pinned engine, the
real harness, and the real durable dependencies. Only the router model boundary
is replaced by a strict scripted implementation. A runner-owned
integration-probe supplies event sinks and an optional controlled target
function; it is outside the subject path and does not add a test-only API to
the engine or harness.

Missing required infrastructure is a setup_error, never a skip.

The implementation is the harness-integration package under
[harness/tests/e2e](https://github.com/iii-hq/workers/tree/main/harness/tests/e2e).
It is a member of the workspace in
[harness/Cargo.toml](https://github.com/iii-hq/workers/blob/main/harness/Cargo.toml)
and uses that workspace lockfile. It is not a nested standalone workspace.

There are two drivers:

| Driver | Owner of the stimulus | Purpose |
|---|---|---|
| direct | The runner synchronously invokes harness::send | Required headless integration scenarios |
| playground | A person or Playwright sends through the production Console or SDK | Console and multi-turn coverage |

The direct runner never uses SDK Enqueue or Void as a substitute for
harness::send. The harness still owns its internal queueing and lifecycle
notifications, so the real durable turn loop remains under test.

## Decisions

| Area | Version 1 decision |
|---|---|
| Isolation | One fresh process stack per scenario; selected scenarios run serially |
| Model boundary | A scripted router owns the six fixed router functions used by the harness |
| Durable dependencies | Real queue, session-manager, context-manager, iii-directory, state, configuration, stream, and cron |
| Readiness | Event-driven harness::ready plus engine::functions-available |
| Completion | harness::turn-completed awakens the runner; durable status and traces confirm the result |
| Oracle | Code assertions over send response, status, transcript, router calls, traces, lifecycle spans, and process state |
| Traces | Normative evidence for target execution and lifecycle delivery |
| Observability | iii-observability is required, in-memory, live, and sampled at 100 percent |
| Authoring | One Rust module per scenario using a small typed DSL that builds runtime contracts directly |
| Reports | Stable result.json plus volatile execution.json linked by SHA-256 |
| Engine acquisition | Explicit --engine-bin or III_BIN; the runner never downloads an engine |
| Repetition | No retry or repeat mode; every selected scenario runs once with a fresh stack |
| Stack reuse | Prohibited |

## Functional requirements

1. Exercise the public path through harness::send, the real queue, and the
   durable harness turn loop.
2. Produce deterministic model behavior without credentials or external
   network access.
3. Detect missing or duplicate transcript entries, router generations,
   function side effects, and lifecycle deliveries.
4. Correlate every terminal turn with one complete, clean session trace.
5. Preserve structured evidence for contract failures, timeouts, process
   crashes, setup failures, and runner failures.
6. Tear down every child process within one bounded cleanup budget.
7. Support production Console stimulus without moving browser automation into
   the Rust runner.

## Boundaries and non-goals

- The suite does not evaluate prompt quality, model judgment, or aesthetic
  output. See [agent-quality.md](agent-quality.md) for that track.
- A test that seeds private harness turn state or invokes an internal
  continuation is not a public-path integration test.
- The scripted router mirrors only the router surface consumed by these
  scenarios. It does not claim to test llm-router routing or providers.
- The integration-probe is runner infrastructure. Its controlled function
  returns a fixed response; target-call and lifecycle evidence come from
  engine traces, not from a private recorder database.
- The direct driver is browserless. The playground driver starts the
  production Console, but a person or external Playwright process owns UI
  interaction and rendering assertions.
- Restart, redelivery, hold/release, fault barriers, approval, cancellation,
  and crash-recovery scenarios are not part of version 1.
- The runner does not download an engine, worker, model, or fixture during a
  scenario.

## System profile

The profile is named harness-core-v1 and is recorded in stack.json with its
component groups, binary paths, binary SHA-256 values, engine lock provenance,
environment allowlist, and start order.

| Component | Profile | Purpose |
|---|---|---|
| iii engine and worker manager | Real, pinned source build | Function registration, triggers, channels, worker lifecycle |
| configuration | Real, isolated filesystem | Authoritative worker configuration |
| iii-state | Real, isolated filesystem | Durable harness state |
| iii-stream | Real | Stream channels |
| iii-cron | Real | Packaged dependency; no fixture schedules are armed |
| iii-observability | Real, in-memory | Live trace storage and query functions |
| queue | Real, file-backed and isolated | Provision and deliver harness-turn work |
| iii-directory | Real, isolated skills directory | Harness dependency and discovery support |
| session-manager | Real, isolated data directory | Session and transcript durability |
| context-manager | Real, isolated lease directory | Context assembly and token preflight |
| harness | Real | System under test |
| scripted-router | Controlled in-process service | Deterministic router model boundary |
| integration-probe | Controlled in-process service | Readiness, registry, trace, lifecycle, and target endpoints |
| Console | Real, playground only | Production browser surface |
| llm-router and providers | Absent | Prevent duplicate router ownership and external inference |
| shell and web workers | Absent | Outside the version 1 corpus |

The engine configuration enables the observability memory exporter with live
spans, a 10,000-span capacity, and a sampling ratio of 1.0. Production
llm-router and provider workers are intentionally not started.

The external worker start order is queue, iii-directory, session-manager, and
context-manager. The runner then arms its controlled services and starts the
harness. Console starts only after the playground stack is ready.

Only PATH, HOME, LANG, and RUST_LOG are forwarded from the parent environment.
Every durable path is allocated under the run root, and the engine port is
reserved independently for each run.

## Platform contracts consumed

| Contract | Integration rule |
|---|---|
| harness::send | Direct scenarios invoke the public function synchronously and persist the exact request and response |
| harness::status | Terminal status must be completed with no pending function calls, queued messages, or child sessions |
| session::messages | The runner follows next_cursor until absent and rejects repeated cursors |
| harness::ready | Empty strict trigger config; payload status is ready and timestamp is an integer |
| harness::turn-completed | Bound by session_id; terminal payload shape is strict and identical retries are accepted |
| engine::functions-available | Supplies the current registry snapshot after registration, replacement, or removal |
| trace | Wakes trace collection when trace_ids change |
| engine::traces::group_by | Selects all trace IDs whose iii.session.id equals the scenario session |
| engine::traces::tree | Returns the complete tree for each selected trace |
| router functions | The scripted service owns exactly the fixed IDs required by the harness path |
| stream events | Frames use the router AssistantMessageEvent vocabulary and end in one terminal done or error event |
| engine call metadata | Top-level keys beginning with an underscore are removed before target and lifecycle payload comparison |

The runner does not use broad descriptor polling as readiness. In particular,
it does not require engine::functions::info checks for every request and
response schema, configuration seed queries, or engine::queue::list_topics.
Mirrored wire contracts are instead pinned by typed structures, JSON Schema
tests, serialization round trips, and producer goldens.

## Architecture

~~~mermaid
sequenceDiagram
  participant R as integration runner
  participant E as iii engine
  participant P as integration-probe
  participant Q as queue
  participant H as harness
  participant X as scripted-router
  participant S as session-manager
  participant O as iii-observability

  R->>R: allocate run root, port, stores, and deadlines
  R->>E: boot isolated base stack
  R->>P: register sinks and optional controlled target
  P->>E: bind functions, trace, lifecycle, and ready triggers
  R->>H: start harness
  H-->>P: harness::ready
  E-->>P: engine::functions-available
  R->>P: confirm active completion binding
  R->>H: harness::send or wait for external stimulus
  H->>Q: enqueue harness-turn
  Q->>H: durable turn step
  H->>S: persist messages
  H->>X: router::chat
  X-->>H: ordered AssistantMessageEvent frames
  H-->>P: harness::turn-completed
  E-->>P: trace changed
  R->>H: harness::status
  R->>S: session::messages, all pages
  R->>O: group session traces and fetch trees
  R->>R: grade, teardown, and report
~~~

The probe binds observers before the harness starts. Trigger registration may
initially be deferred while custom trigger types do not yet exist. Once
readiness proves that the harness registered them, the runner creates an
acknowledged active harness::turn-completed binding and removes the stale
deferred completion binding before any direct stimulus.

## Lifecycle and readiness

The harness owns two custom trigger types:

- harness::ready accepts an empty configuration object and emits
  {status: ready, timestamp: now_ms} after boot initialization, queue
  provisioning, lifecycle registration, and configuration bindings complete.
- harness::turn-completed accepts a strict session filter and emits the
  terminal turn payload after durable state is persisted.

The ready trigger retains ready state. Subscribers already bound receive the
boot emission; a binding registered after the harness is ready receives an
immediate delivery.

Turn-completed delivery is Void, at-least-once, and unordered. Its required
fields are session_id, turn_id, status, terminal, and timestamp. Status is one
of completed, cancelled, or failed. Optional fields are result, result_error,
reason, parent, parent_session_id, and reactive_depth; parent contains
session_id, turn_id, and function_call_id.

The integration-probe registers four sink functions:

- integration-probe::harness-ready
- integration-probe::functions-changed
- integration-probe::traces-changed
- integration-probe::turn-completed

Readiness requires both the harness::ready observation and a current function
snapshot containing at least:

- harness::send
- session::messages
- context::assemble

All waits and RPCs have monotonic deadlines. Failure before stimulus, including
a readiness deadline, is setup_error. Once the direct send or external
playground turn is active, expiration of the scenario deadline is timeout.

## Scripted router

The scripted router and production llm-router are mutually exclusive. It owns
these six function IDs:

- router::chat
- router::abort
- router::models::list
- router::models::get
- router::models::supports
- router::system_prompt::get

Models list, get, and supports are deterministic projections of the fixture
model. Models get accepts an omitted or empty provider and resolves by model ID
alone; when a provider is present it must match. System prompt get identifies
the scripted provider and omits a provider prompt so the harness uses
[harness/prompts/default.txt](https://github.com/iii-hq/workers/blob/main/harness/prompts/default.txt).
Abort closes a live generation once and reports whether it actually aborted
one.

Every router::chat generation explicitly matches all twelve request fields, in
canonical order:

1. writer_ref
2. request_id
3. model
4. provider
5. system_prompt
6. messages
7. tools
8. response_format
9. thinking_level
10. max_output_tokens
11. provider_options
12. metadata

Supported JSON matchers are absent, regex, sha256, exact, and subset. Exact and
subset may normalize values only by deleting RFC 6901 JSON Pointer locations.
There is no present matcher and no replace normalizer.

The fixture model contains:

- id and provider
- context_window and max_output_tokens
- optional supports_thinking, supports_xhigh, supports_tools,
  supports_vision, supports_cache, and supports_structured_output

The scripted response constructors cover plain text, streamed text, and native
function calls. Streamed text uses the router's 15 event variants:

- start, text_start, text_delta, text_end
- thinking_start, thinking_delta, thinking_end
- functioncall_start, functioncall_delta, functioncall_end
- usage, ping, stop, done, error

The version 1 streamed fixture emits start, text_start, one or more text_delta
events, text_end, usage, stop, and done. Exactly one terminal frame is allowed,
and terminal message, stop reason, provider, model, and usage must agree with
the RouterChatResponse. The fixture DSL does not expose arbitrary raw response
or frame escape hatches.

RouterChatResponse contains ok, provider, and model, with optional stop_reason,
usage, and error. The scripted function returns it only after the ordered
stream frames have been relayed.

## Scenario authoring and expansion

Each scenario is a Rust module under
[harness/tests/e2e/src/scenarios](https://github.com/iii-hq/workers/tree/main/harness/tests/e2e/src/scenarios).
The typed DSL keeps the send policy, request matchers, scripted responses,
controlled function, expected history, and verification together at the call
site.

Builders compile directly to runtime structures. There is no YAML layer,
generic authored-scenario compiler, committed compiled snapshot directory, or
checked-in schemas directory.

The in-memory CompiledFixtureV1 groups the compiled scenario, router script,
and system prompt template. ScenarioFixture adds runner-only slug, driver,
expected terminal-turn count, and verification function.

CompiledScenarioV1 contains:

| Field | Contract |
|---|---|
| schema_version | Literal version 1 |
| id | 1 to 128 ASCII letters, digits, hyphen, or underscore |
| description | Human-readable scenario intent |
| send | Narrow typed harness::send request |
| target | Optional controlled target |
| deadlines | readiness_ms, scenario_ms, teardown_ms |

CompiledSendV1 contains session_id, message, model, provider,
idempotency_key, and an options.functions object. The functions object has
allow and deny arrays and expose fixed to native.

ControlledTargetV1 contains function_id, description, an object request_schema,
and a fixed JSON response. Its function ID must be prefixed by the concrete
run ID before registration.

Default deadlines are:

| Phase | Milliseconds |
|---|---:|
| readiness | 60,000 |
| scenario | 60,000 |
| teardown | 15,000 |

Before boot, expansion replaces:

- {{run_id}}
- {{session_id}}
- {{system_prompt_sha256}}

The expanded expected system prompt is persisted as
expected-system-prompt.txt. Fixture validation occurs before process startup;
an invalid fixture is runner_error, not a subject failure.

## Probe and trace evidence

The integration-probe is in-process runner infrastructure. It registers the
optional controlled function directly with the SDK and returns the fixture's
fixed response. It does not maintain a target-call log or a durable lifecycle
event store.

The completion sink stores only enough in-memory information to awaken the
runner and remember the trace generation observed at completion. Normative
function-call and lifecycle evidence is reconstructed from traces:

- a controlled call is a span named execute <run_id>::<alias>
- a lifecycle delivery is a span named
  execute integration-probe::turn-completed
- invocation input and output come from iii.invocation.input and
  iii.invocation.output span events
- payload JSON comes from iii.payload.json and must not be marked truncated

After completion, the collector waits for a later trace change, calls
engine::traces::group_by with attribute iii.session.id, limit 100, and
include_internal true, then fetches every tree through engine::traces::tree.
Trace IDs and spans are sorted deterministically before grading and
persistence.

Trace evidence is stable only when:

- no span is pending
- the number of distinct turn IDs equals expected_terminal_turns
- every expected terminal turn has a lifecycle sink span
- lifecycle turn IDs belong to the collected session traces

The grading floor separately requires the number of trace trees to equal
expected_terminal_turns.

TraceEvidenceV1 persists complete trees and this summary:

- trace_count
- span_count
- error_count
- pending_span_count
- ordered turn_ids

## Common grading floor

Every scenario first passes the same runner-owned floor:

1. Final status is completed.
2. pending_function_calls, queued, and children are empty.
3. Trace count and distinct turn count equal expected_terminal_turns.
4. No trace span is pending or in error.
5. The latest collected turn ID equals the active turn ID.
6. Lifecycle trace inputs are complete, use only the published payload fields,
   are terminal and completed, and bind to the scenario session and trace turn.
7. Repeated lifecycle deliveries are allowed only when their payloads are
   identical after timestamp removal; conflicting terminal payloads fail.
8. Every scripted router generation is consumed, with no missing or extra
   generation.
9. For direct scenarios, send is accepted and merged, queued, and deduplicated
   are false or absent.

The scenario-specific verifier then checks transcript content, message counts,
controlled call count and payload, function history, and duplicate entry IDs.
The first floor or scenario failure is retained.

## Oracles

| Oracle | Verified property |
|---|---|
| Send response | Acceptance, returned turn identity, and clean merge, queue, and deduplication flags |
| Final status | Durable completion and absence of pending work |
| Full transcript | Ordered durable user, assistant, and function-result messages with no duplicate entry IDs |
| Scripted router evidence | Exact request shape, model-visible history, tools, generation order, and complete consumption |
| Session trace trees | Turn count, clean completion, controlled target calls, lifecycle delivery, and payloads |
| Process supervisor | Unexpected exits and bounded shutdown |
| Teardown report | Signal, reap, and cleanup outcome for every process group |

Private state, logs, and raw stack files are diagnostic only and cannot turn a
failing public oracle into a pass.

## Version 1 scenario corpus

The registry contains exactly four scenarios:

| ID | Slug | Driver | Terminal turns | Core invariant |
|---|---|---|---:|---|
| E2E-001 | streamed-text | direct | 1 | Streamed text reaches durable completion |
| E2E-002 | exactly-once-function | direct | 1 | One allowed native function executes exactly once |
| UI-001 | console-streamed-text | playground | 1 | A Console-originated message streams to durable completion |
| UI-002 | multi-turn-traces | playground | 2 | A function turn and a Console turn produce distinct complete traces |

Validate all checks all four fixtures. Run all executes only direct scenarios;
playground scenarios must be selected individually with the playground
command.

### E2E-001 — streamed text

The direct request sends Return the fixture phrase. to fixture-model through
the scripted provider, with no allowed functions and an idempotency key scoped
to the run.

The router expects one user message, no tools, the SHA-256 of the built-in
system prompt, and the first generated request ID. It streams fixture complete
in two deltas with usage input 8 and output 2.

Required scenario assertions:

- exactly one user and one assistant durable message
- assistant text equals fixture complete
- no function-result message
- no duplicate transcript entry ID
- exactly one router generation consumed

### E2E-002 — exactly-once native function

The direct request sends Call the recorder once. and allows one run-scoped
function named <run_id>::record. The controlled target has an exact object
schema requiring the string field value and returns a successful text result
containing recorded.

Generation one emits native call call-1 with {value: expected}, usage input 8
and output 4. Generation two must see, in order, the original user message, the
assistant function call, and the durable function result. It returns recorded
once with usage input 18 and output 2.

Required scenario assertions:

- the trace contains exactly one execute <run_id>::record span
- its normalized invocation input equals {value: expected}
- the second router request contains the exact function-call and
  function-result history
- final assistant text equals recorded once
- no duplicate transcript entry ID
- exactly two router generations consumed

The word recorder remains fixture wording only; there is no recorder service
or recorder event schema.

### UI-001 — Console streamed text

The playground creates a Console session and waits for a person or Playwright
to submit Return the console fixture phrase. The router matcher permits the
Console-specific system prompt through an agent_trigger regex and requires the
tools collection to contain agent_trigger.

The response streams console fixture complete. The Rust verifier enforces the
common floor and absence of duplicate transcript entries; browser rendering
assertions belong to the external Playwright test.

### UI-002 — two turns and traces

The playground expects two terminal turns. The external driver owns both
stimuli:

1. Invoke the compiled send from playground-ready.json. This performs the same
   controlled function flow as E2E-002 and finishes with recorded once.
2. Submit Return the second trace phrase. through the Console. This streams
   second trace complete.

The first turn consumes request steps 0 and 1. The Console turn starts a new
turn at request step 0. Required assertions are two complete traces, two user
messages, three assistant messages including the function-call assistant
entry, one function result, one controlled target execution, the exact target
payload, both final assistant texts, and no duplicate transcript entries.

## CLI

The harness-integration binary exposes:

| Command | Selection | Purpose |
|---|---|---|
| run | ID, slug, or all | Execute one or all direct scenarios |
| validate | ID, slug, or all | Compile and validate registered fixtures without booting |
| playground | One ID or slug | Boot one playground scenario and production Console |

Shared stack arguments:

- --engine-bin, falling back to III_BIN
- optional --harness-bin
- repeated --worker-bin name=path

Run additionally supports --artifacts-dir, defaulting to target/integration,
and --retain-success. Playground requires --console-bin, supports optional
--ready-file, and defaults to target/console-e2e. Playground rejects all
because it owns one isolated interactive stack. Scenario selection defaults to
all; run rejects an explicitly selected playground scenario.

Make entry points are:

~~~sh
make -C harness integration-validate
make -C harness integration-e2e III_BIN=/path/to/iii
make -C harness integration-playground III_BIN=/path/to/iii
~~~

There is no --repeat option and no attempt subdirectory.

## Playground handoff

After the stack, session, and production Console are ready, the runner writes
playground-ready.json and optionally publishes the same JSON atomically to
--ready-file.

PlaygroundReadyV1 contains:

- schema version, run ID, scenario ID, slug, and driver
- run root and result path
- engine and Console URLs
- session ID and title
- model ID and provider
- fixture message
- controlled function aliases mapped to expanded IDs
- the fully expanded direct send

There is no separate start signal. Depending on the scenario, Playwright uses
the compiled send through the SDK or interacts through the Console UI. The
runner waits for the first terminal completion and then remains active until
SIGINT or SIGTERM. On shutdown it requires every terminal turn declared by the
fixture, binds evidence to the latest one, and refreshes durable status. If
shutdown arrives just before the first completion callback, it allows a
one-second grace period; stopping before that completion is a contract failure.

PlaygroundResultV1 contains classification, failure, a compact evidence object,
and artifact paths. Complete trace trees remain in traces.json.

## Process supervision and teardown

Every child runs in its own process group. The supervisor:

- waits up to ten seconds for the engine listener, probing every 25
  milliseconds
- retries engine startup once only for an explicit bind race after complete
  teardown
- observes Unix SIGCHLD for event-driven early-exit detection, with a polling
  fallback on non-Unix platforms
- treats an unexpected exit before teardown as process_crash
- tears down in reverse start order
- sends SIGTERM, then SIGKILL when needed, within one hard teardown budget
- records typed signal, wait, reap, and deadline issues in teardown.json

Incomplete teardown cannot pass. Process-exit.json is written when a subject
process exits unexpectedly and references its stderr log.

## Classification and reports

Classifications, in increasing precedence, are:

1. pass
2. contract_failure
3. timeout
4. setup_error
5. process_crash
6. runner_error

A readiness failure is setup_error. A subject completion deadline in Await is
timeout. Evidence collection, artifact persistence, or teardown failures are
runner_error. A harness::send RPC failure in Send is contract_failure even if
the transport error text mentions a timeout. Classification does not replace
underlying engine or SDK error details in failure artifacts.

Process exit codes are:

| Exit | Meaning |
|---:|---|
| 0 | Every selected scenario passed |
| 2 | At least one contract_failure or timeout |
| 3 | At least one setup_error, process_crash, or runner_error |

IntegrationResultV1 is the stable, byte-comparable result:

- schema_version
- scenario_id
- classification
- optional failure string
- artifact path list

Concrete run, session, and turn IDs in failure text are replaced by
{{run_id}}, {{session_id}}, and {{turn_id}}. IntegrationResultV1 has no
invariants array.

ExecutionReportV1 is intentionally volatile:

- schema_version
- run_id
- scenario_id
- started_at
- duration_ms
- result_path
- result_sha256

The digest covers the exact result.json bytes. There is no attempt field.

## Repository and artifacts

Relevant source layout:

~~~text
harness/
  Cargo.toml
  Cargo.lock
  tests/e2e/
    Cargo.toml
    README.md
    engine.lock
    src/
      main.rs
      artifacts.rs
      probe.rs
      scripted_router.rs
      trace_evidence.rs
      scenarios/
        dsl.rs
        streamed_text.rs
        exactly_once_function.rs
        console_streamed_text.rs
        multi_turn_traces.rs
      scenario/
        floor.rs
        playground.rs
        phases/
      stack/
      process/
      types/
    tests/
      determinism.rs
      scenario_compilation.rs
      schemas.rs
      supervisor.rs
~~~

A direct run writes:

~~~text
target/integration/<run_id>/
  result.json
  execution.json
  stack.json
  teardown.json
  process-exit.json                 # only after an unexpected exit
  logs/
  engine/
  seeds/
  session-data/
  leases/
  queue/
  skills/
  scenarios/<scenario_id>/
    expected-system-prompt.txt
    request.json
    send-response.json
    transcript.json
    status.json
    router-calls.json
    traces.json
    failure.json                    # only after failure
~~~

Playground uses an analogous tree under target/console-e2e/<run_id>/. It writes
playground-ready.json and playground-result.json instead of result.json and
execution.json. It retains stack.json, teardown.json, logs, stack state, the
expected prompt, transcript, status, router calls, traces, and an optional
failure. It does not write request.json or send-response.json because the Rust
runner does not issue the stimulus.

On a direct pass, unless --retain-success is set, the runner removes engine,
logs, seeds, session-data, leases, skills, and queue state while retaining
compact reports and collected scenario evidence. Playground always performs
the same trimming on pass. Non-pass runs keep the full stack state.

## CI and gate policy

The harness-integration job in
[.github/workflows/ci.yml](https://github.com/iii-hq/workers/blob/main/.github/workflows/ci.yml)
runs on every pull request and workflow_dispatch. There is no scheduled
trigger. It is currently non-required through branch protection; the job
itself does not use continue-on-error.

The engine pin in
[harness/tests/e2e/engine.lock](https://github.com/iii-hq/workers/blob/main/harness/tests/e2e/engine.lock)
is:

~~~toml
repository = "iii-hq/iii"
revision = "15dc993ebfdbfcabe5d299cf4cae4dd676db4c06"
package = "iii"
binary = "target/release/iii"
~~~

CI:

1. Validates that engine.lock contains exactly the four supported fields and a
   40-hex revision.
2. Checks out the exact engine source under target/integration-engine-src.
3. Builds the pinned package with Cargo locked in release mode.
4. Caches the build by operating system, architecture, revision, and engine
   Cargo.lock hash, then re-hashes the resulting binary.
5. Runs the harness-integration unit and contract tests.
6. Runs integration-validate.
7. Runs E2E-001 and E2E-002 once each through integration-e2e.
8. Verifies every execution report against the exact result.json SHA-256.
9. Always uploads result.json, execution.json, and teardown.json.
10. On failure, uploads the complete target/integration tree for 14 days.

There is no CI repeat pass, byte-comparison loop between attempts, scheduled
promotion window, or automatic engine download fallback.

## Verification and acceptance

The implementation is accepted when:

- fixture validation registers exactly E2E-001, E2E-002, UI-001, and UI-002
  once each
- validate all checks all four while run all selects exactly the two direct
  scenarios
- serde round trips cover compiled scenario, compiled fixture, router script,
  trace evidence, stable result, and execution report
- the generated CompiledScenarioV1 JSON Schema enforces safe scenario IDs and
  positive deadlines
- producer goldens pin the scripted router and authoritative harness::send
  request shape
- frame tests enforce one terminal event and agreement with the terminal
  router response
- router matcher tests cover all twelve fields and only the supported
  normalization behavior
- probe tests reject malformed ready, function-registry, trace, and lifecycle
  payloads
- trace and floor tests cover pending spans, error spans, target payload
  extraction, lifecycle shape, and conflicting duplicate deliveries
- supervisor tests cover early exits, environment isolation, process groups,
  cooperative shutdown, and SIGKILL escalation
- classification tests enforce phase-sensitive timeout and error precedence
- result tests enforce stable scrubbed bytes and execution digest linkage
- E2E-001 and E2E-002 pass through fresh real stacks without a model key
- playground produces a complete automation handoff for UI-001 and UI-002

The package test command is:

~~~sh
cargo test --locked --manifest-path harness/Cargo.toml -p harness-integration
~~~

## Post-version-1 expansion

Future scenarios may add denied functions, repeated-send idempotency, router
failure and retry, structured output repair, steering, hooks, approval,
sub-agents, cancellation, queue redelivery, restart recovery, dynamic
registration, and runtime validation.

Multi-send automation should extend the typed scenario contract with ordered
public send and await steps. Fault injection should use separately versioned
test-support contracts. Neither should introduce private harness state
mutation into the public-path suite.

## Related material

- [Harness E2E runner README](https://github.com/iii-hq/workers/blob/main/harness/tests/e2e/README.md)
- [Harness E2E tests](agent-quality.md)
- [Harness architecture](https://github.com/iii-hq/workers/blob/main/harness/architecture/README.md)
- [harness::send](https://github.com/iii-hq/workers/blob/main/harness/src/functions/send.rs)
- [Harness lifecycle and readiness](https://github.com/iii-hq/workers/blob/main/harness/src/events.rs)
- [Durable turn loop](https://github.com/iii-hq/workers/blob/main/harness/src/turn_loop.rs)
- [Queue provisioning](https://github.com/iii-hq/workers/blob/main/harness/src/queue.rs)
- [Scripted router types](https://github.com/iii-hq/workers/blob/main/harness/tests/e2e/src/types/script.rs)
- [Trace evidence](https://github.com/iii-hq/workers/blob/main/harness/tests/e2e/src/trace_evidence.rs)
- [iii core primitives](../../skills/iii-core-primitives/SKILL.md)
- [iii engine configuration](../../skills/iii-engine-config/SKILL.md)
- [iii error handling](../../skills/iii-error-handling/SKILL.md)
