# Requirements: iii Queue & HTTP Invoker Hardening

**Defined:** 2026-03-16
**Core Value:** Every enqueued job must be reliably processed or land in DLQ with full traceability, and every HTTP invocation must succeed or fail with clear, actionable errors — no silent data loss.

## v1 Requirements

Requirements for this hardening initiative. Each maps to roadmap phases.

### Test Infrastructure

- [x] **INFRA-01**: Shared test helpers extracted from duplicated code in queue_integration.rs and rabbitmq_queue_integration.rs into tests/common/ module
- [x] **INFRA-02**: wiremock added as dev-dependency for mock HTTP server in E2E tests
- [x] **INFRA-03**: testcontainers and testcontainers-modules (rabbitmq feature) added as dev-dependencies
- [x] **INFRA-04**: tokio test-util feature enabled for deterministic time control via start_paused

### Queue E2E Tests (Builtin)

- [x] **QBLT-01**: E2E test verifies enqueue → process → ack happy path with payload integrity
- [x] **QBLT-02**: E2E test verifies retry with exponential backoff timing (using start_paused for deterministic verification)
- [x] **QBLT-03**: E2E test verifies DLQ exhaustion with content verification (payload, error reason, attempt count — not just count)
- [x] **QBLT-04**: E2E test verifies FIFO ordering under concurrent load with multiple message groups
- [x] **QBLT-05**: E2E test verifies max_retries=0 boundary sends directly to DLQ without processing
- [x] **QBLT-06**: E2E test verifies handler panic does not crash the worker and message is retried or DLQ'd
- [x] **QBLT-07**: E2E test verifies condition-based message filtering routes only matching messages

### Queue E2E Tests (RabbitMQ)

- [x] **QRMQ-01**: E2E test verifies enqueue → process → ack via RabbitMQ adapter using testcontainers
- [x] **QRMQ-02**: E2E test verifies retry with backoff via RabbitMQ TTL-based retry mechanism
- [x] **QRMQ-03**: E2E test verifies DLQ exhaustion via RabbitMQ dead letter exchange with content verification
- [x] **QRMQ-04**: E2E test verifies FIFO ordering via RabbitMQ with message group support
- [x] **QRMQ-05**: E2E test verifies max_retries=0 boundary via RabbitMQ adapter
- [x] **QRMQ-06**: E2E test verifies handler panic recovery via RabbitMQ adapter
- [x] **QRMQ-07**: E2E test verifies RabbitMQ-specific exchange routing and topology setup

### HTTP Invoker E2E Tests

- [x] **HTTP-01**: E2E test verifies timeout behavior when remote endpoint is slow (response exceeds configured timeout)
- [x] **HTTP-02**: E2E test verifies connection refused handling returns clear error with actionable message
- [x] **HTTP-03**: E2E test verifies structured error response parsing (error.code, error.message from JSON)
- [x] **HTTP-04**: E2E test verifies unstructured error response fallback to HTTP status message
- [x] **HTTP-05**: E2E test verifies empty response body (204 No Content) returns None successfully
- [x] **HTTP-06**: E2E test verifies private IP blocking rejects requests to RFC1918 addresses
- [x] **HTTP-07**: E2E test verifies HTTPS enforcement rejects HTTP URLs when require_https is enabled
- [x] **HTTP-08**: E2E test verifies DNS failure handling returns SecurityError::DnsLookupFailed

### Bug Fixes

- [x] **FIX-01**: Backoff calculation uses saturating arithmetic to prevent u64 overflow at high retry counts (attempts >= 54)
- [x] **FIX-02**: Delayed-to-waiting queue transition is atomic to prevent duplicate job delivery under concurrent load
- [ ] **FIX-03**: Auth E2E tests refactored to avoid unsafe env::set_var pattern with safer alternative

### Backlog Documentation

- [ ] **BLOG-01**: Backlog document created with future reliability improvements (circuit breaker, body size limits, consumer crash recovery, message deduplication, DNS rebinding fix)
- [ ] **BLOG-02**: Backlog document includes future feature additions (priority queues, scheduled jobs UI, retry policies, Redis adapter hardening)

## v2 Requirements

Deferred to future release. Tracked but not in current roadmap.

### Security Hardening

- **SEC-01**: DNS rebinding SSRF mitigation via reqwest custom resolver to pin validated IPs
- **SEC-02**: Request/response body size limits to prevent resource exhaustion
- **SEC-03**: HMAC replay protection guidance and optional timestamp freshness enforcement

### Reliability

- **REL-01**: Circuit breaker for HTTP invocations to failing endpoints
- **REL-02**: Consumer crash recovery with automatic task restart
- **REL-03**: Message deduplication with configurable retention window
- **REL-04**: Redis adapter E2E test parity with builtin and RabbitMQ

### Features

- **FEAT-01**: Priority queues with configurable priority levels
- **FEAT-02**: DLQ redrive API for operational recovery
- **FEAT-03**: HTTP invoker retry policies (configurable retry count, backoff strategy)

## Out of Scope

| Feature | Reason |
|---------|--------|
| Redis adapter hardening | Not actively used; focus on builtin + RabbitMQ |
| Performance benchmarking | Separate initiative — correctness first |
| SDK-level changes | Focus is on engine reliability |
| New queue features (priority, scheduling) | Backlog — correctness before features |
| HTTP invoker auth type E2E tests | Auth already has 58 unit tests; focus E2E on error/security paths |

## Traceability

Which phases cover which requirements. Updated during roadmap creation.

| Requirement | Phase | Status |
|-------------|-------|--------|
| INFRA-01 | Phase 1 | Complete |
| INFRA-02 | Phase 1 | Complete |
| INFRA-03 | Phase 1 | Complete |
| INFRA-04 | Phase 1 | Complete |
| QBLT-01 | Phase 2 | Complete |
| QBLT-02 | Phase 3 | Complete |
| QBLT-03 | Phase 3 | Complete |
| QBLT-04 | Phase 4 | Complete |
| QBLT-05 | Phase 3 | Complete |
| QBLT-06 | Phase 4 | Complete |
| QBLT-07 | Phase 2 | Complete |
| QRMQ-01 | Phase 7 | Complete |
| QRMQ-02 | Phase 8 | Complete |
| QRMQ-03 | Phase 8 | Complete |
| QRMQ-04 | Phase 9 | Complete |
| QRMQ-05 | Phase 8 | Complete |
| QRMQ-06 | Phase 9 | Complete |
| QRMQ-07 | Phase 7 | Complete |
| HTTP-01 | Phase 5 | Complete |
| HTTP-02 | Phase 5 | Complete |
| HTTP-03 | Phase 5 | Complete |
| HTTP-04 | Phase 5 | Complete |
| HTTP-05 | Phase 5 | Complete |
| HTTP-06 | Phase 6 | Complete |
| HTTP-07 | Phase 6 | Complete |
| HTTP-08 | Phase 6 | Complete |
| FIX-01 | Phase 3 | Complete |
| FIX-02 | Phase 4 | Complete |
| FIX-03 | Phase 10 | Pending |
| BLOG-01 | Phase 10 | Pending |
| BLOG-02 | Phase 10 | Pending |

**Coverage:**
- v1 requirements: 31 total
- Mapped to phases: 31
- Unmapped: 0

---
*Requirements defined: 2026-03-16*
*Last updated: 2026-03-16 after roadmap creation*
