# Roadmap: iii Queue & HTTP Invoker Hardening

## Overview

This roadmap delivers comprehensive E2E test coverage and edge case fixes for the iii engine's queue system (builtin and RabbitMQ adapters) and HTTP invoker. The work flows from test infrastructure setup through progressive E2E coverage of each subsystem, fixing bugs alongside the tests that expose them, and concludes with cleanup and backlog documentation. Every phase delivers a verifiable testing capability.

## Phases

**Phase Numbering:**
- Integer phases (1, 2, 3): Planned milestone work
- Decimal phases (2.1, 2.2): Urgent insertions (marked with INSERTED)

Decimal phases appear between their surrounding integers in numeric order.

- [ ] **Phase 1: Test Infrastructure** - Shared helpers, dev-dependencies, and deterministic time control
- [ ] **Phase 2: Builtin Queue Happy Path** - E2E tests for enqueue-process-ack and condition filtering
- [ ] **Phase 3: Builtin Queue Failure & Retry** - E2E tests for retry, backoff, DLQ exhaustion, and backoff overflow fix
- [ ] **Phase 4: Builtin Queue Concurrency & Resilience** - E2E tests for FIFO ordering, panic recovery, and atomic queue transition fix
- [ ] **Phase 5: HTTP Invoker Error Handling** - E2E tests for timeouts, connection errors, and response parsing
- [ ] **Phase 6: HTTP Invoker Security** - E2E tests for private IP blocking, HTTPS enforcement, and DNS failure
- [ ] **Phase 7: RabbitMQ Happy Path & Topology** - E2E tests for RabbitMQ enqueue-process-ack and exchange routing
- [ ] **Phase 8: RabbitMQ Failure & Retry** - E2E tests for RabbitMQ TTL-based retry, DLQ exhaustion, and boundary cases
- [ ] **Phase 9: RabbitMQ Concurrency & Resilience** - E2E tests for RabbitMQ FIFO ordering and panic recovery
- [ ] **Phase 10: Auth Cleanup & Backlog Documentation** - Auth test env safety fix and future improvement backlog

## Phase Details

### Phase 1: Test Infrastructure
**Goal**: All shared test utilities and dependencies are in place so subsequent phases can write E2E tests without duplicating setup code
**Depends on**: Nothing (first phase)
**Requirements**: INFRA-01, INFRA-02, INFRA-03, INFRA-04
**Success Criteria** (what must be TRUE):
  1. A tests/common/ module exists with shared helpers extracted from queue_integration.rs and rabbitmq_queue_integration.rs, and both existing test files compile against it
  2. Running `cargo test` with wiremock, testcontainers, and testcontainers-modules (rabbitmq) as dev-dependencies compiles without errors
  3. A test using `#[tokio::test(start_paused = true)]` can advance time deterministically and passes in CI
**Plans**: 2 plans

Plans:
- [ ] 01-01-PLAN.md — Create shared test helpers module, add dev-dependencies, migrate queue_integration.rs
- [ ] 01-02-PLAN.md — Implement RabbitMQ testcontainers helpers and migrate rabbitmq_queue_integration.rs

### Phase 2: Builtin Queue Happy Path
**Goal**: The builtin queue adapter's core success flows are verified end-to-end with payload integrity and message routing
**Depends on**: Phase 1
**Requirements**: QBLT-01, QBLT-07
**Success Criteria** (what must be TRUE):
  1. An E2E test enqueues a message, processes it via a handler, and verifies the received payload matches the sent payload byte-for-byte
  2. An E2E test enqueues messages with different conditions and verifies only matching messages reach the handler
**Plans**: TBD

Plans:
- [ ] 02-01: TBD

### Phase 3: Builtin Queue Failure & Retry
**Goal**: The builtin queue adapter correctly retries failed messages with exponential backoff, exhausts to DLQ with full traceability, and handles boundary cases without arithmetic overflow
**Depends on**: Phase 2
**Requirements**: QBLT-02, QBLT-03, QBLT-05, FIX-01
**Success Criteria** (what must be TRUE):
  1. An E2E test verifies retry intervals match exponential backoff formula using deterministic time control (start_paused)
  2. An E2E test verifies that after max retries, the message lands in DLQ with correct payload, error reason, and attempt count
  3. An E2E test verifies that max_retries=0 sends the message directly to DLQ without any processing attempt
  4. Backoff calculation at retry attempt 54 and above does not panic or overflow (saturating arithmetic in production code)
**Plans**: TBD

Plans:
- [ ] 03-01: TBD
- [ ] 03-02: TBD

### Phase 4: Builtin Queue Concurrency & Resilience
**Goal**: The builtin queue adapter maintains FIFO ordering under concurrent load and recovers gracefully from handler panics without message loss
**Depends on**: Phase 2
**Requirements**: QBLT-04, QBLT-06, FIX-02
**Success Criteria** (what must be TRUE):
  1. An E2E test enqueues messages across multiple message groups under concurrent load and verifies per-group FIFO ordering is preserved
  2. An E2E test triggers a handler panic and verifies the worker does not crash and the message is retried or moved to DLQ
  3. The delayed-to-waiting queue transition is atomic, verified by a test that cannot produce duplicate job delivery under concurrent access
**Plans**: TBD

Plans:
- [ ] 04-01: TBD
- [ ] 04-02: TBD

### Phase 5: HTTP Invoker Error Handling
**Goal**: The HTTP invoker returns clear, actionable errors for every failure mode a remote endpoint can produce
**Depends on**: Phase 1
**Requirements**: HTTP-01, HTTP-02, HTTP-03, HTTP-04, HTTP-05
**Success Criteria** (what must be TRUE):
  1. An E2E test with a wiremock delayed response verifies the invoker returns a timeout error when the response exceeds the configured timeout
  2. An E2E test verifies connection refused produces a clear error message (not a raw OS error)
  3. An E2E test verifies structured JSON error responses (error.code, error.message) are parsed and surfaced to the caller
  4. An E2E test verifies non-JSON error responses fall back to HTTP status message
  5. An E2E test verifies 204 No Content with empty body returns None successfully
**Plans**: TBD

Plans:
- [ ] 05-01: TBD
- [ ] 05-02: TBD

### Phase 6: HTTP Invoker Security
**Goal**: The HTTP invoker's security validations reject dangerous requests and produce specific, informative error types
**Depends on**: Phase 1
**Requirements**: HTTP-06, HTTP-07, HTTP-08
**Success Criteria** (what must be TRUE):
  1. An E2E test verifies requests to RFC1918 private IP addresses are rejected with a security error
  2. An E2E test verifies HTTP URLs are rejected when require_https is enabled, with a clear error indicating HTTPS is required
  3. An E2E test verifies DNS lookup failure returns SecurityError::DnsLookupFailed (not a generic connection error)
**Plans**: TBD

Plans:
- [ ] 06-01: TBD

### Phase 7: RabbitMQ Happy Path & Topology
**Goal**: The RabbitMQ adapter correctly provisions exchanges, queues, and bindings, and delivers messages end-to-end via a real RabbitMQ broker
**Depends on**: Phase 1
**Requirements**: QRMQ-01, QRMQ-07
**Success Criteria** (what must be TRUE):
  1. An E2E test using testcontainers starts a RabbitMQ broker, enqueues a message, processes it, and verifies payload integrity
  2. An E2E test verifies RabbitMQ exchange routing and topology setup (queues, exchanges, bindings) match the expected configuration
**Plans**: TBD

Plans:
- [ ] 07-01: TBD

### Phase 8: RabbitMQ Failure & Retry
**Goal**: The RabbitMQ adapter's TTL-based retry mechanism correctly retries, backs off, and exhausts to DLQ via the dead letter exchange
**Depends on**: Phase 7
**Requirements**: QRMQ-02, QRMQ-03, QRMQ-05
**Success Criteria** (what must be TRUE):
  1. An E2E test verifies retry with backoff via RabbitMQ TTL-based retry (using real time, not start_paused) with tolerance for timing variance
  2. An E2E test verifies DLQ exhaustion via RabbitMQ dead letter exchange with correct payload and attempt metadata
  3. An E2E test verifies max_retries=0 sends the message directly to DLQ via RabbitMQ without processing
**Plans**: TBD

Plans:
- [ ] 08-01: TBD

### Phase 9: RabbitMQ Concurrency & Resilience
**Goal**: The RabbitMQ adapter maintains FIFO ordering with message groups and recovers from handler panics without losing messages
**Depends on**: Phase 7
**Requirements**: QRMQ-04, QRMQ-06
**Success Criteria** (what must be TRUE):
  1. An E2E test verifies FIFO ordering via RabbitMQ with message group support under concurrent consumers
  2. An E2E test triggers a handler panic with the RabbitMQ adapter and verifies the message is nack'd and retried or moved to DLQ
**Plans**: TBD

Plans:
- [ ] 09-01: TBD

### Phase 10: Auth Cleanup & Backlog Documentation
**Goal**: Unsafe test patterns are removed and all identified future improvements are documented in a structured backlog
**Depends on**: Phase 6
**Requirements**: FIX-03, BLOG-01, BLOG-02
**Success Criteria** (what must be TRUE):
  1. Auth E2E tests no longer use unsafe env::set_var; the safer alternative compiles and all auth tests pass
  2. A backlog document exists listing future reliability improvements (circuit breaker, body size limits, consumer crash recovery, message deduplication, DNS rebinding fix) with rationale for each
  3. The same backlog document includes future feature additions (priority queues, scheduled jobs UI, retry policies, Redis adapter hardening) categorized separately from reliability items
**Plans**: TBD

Plans:
- [ ] 10-01: TBD

## Progress

**Execution Order:**
Phases execute in numeric order: 1 -> 2 -> 3 -> 4 -> 5 -> 6 -> 7 -> 8 -> 9 -> 10
Note: Phases 2-4 (builtin queue) and 5-6 (HTTP invoker) share only Phase 1 as a dependency; they could execute in parallel if desired.

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 1. Test Infrastructure | 0/2 | Planning | - |
| 2. Builtin Queue Happy Path | 0/? | Not started | - |
| 3. Builtin Queue Failure & Retry | 0/? | Not started | - |
| 4. Builtin Queue Concurrency & Resilience | 0/? | Not started | - |
| 5. HTTP Invoker Error Handling | 0/? | Not started | - |
| 6. HTTP Invoker Security | 0/? | Not started | - |
| 7. RabbitMQ Happy Path & Topology | 0/? | Not started | - |
| 8. RabbitMQ Failure & Retry | 0/? | Not started | - |
| 9. RabbitMQ Concurrency & Resilience | 0/? | Not started | - |
| 10. Auth Cleanup & Backlog Documentation | 0/? | Not started | - |
