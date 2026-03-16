# iii Engine Backlog

**Created:** 2026-03-16
**Last reviewed:** 2026-03-16
**Source:** Queue and HTTP Invoker Hardening initiative (Phases 1-9)

This document consolidates all future reliability improvements and feature additions identified during the hardening initiative into a single structured reference. Each item was discovered during E2E test development, codebase analysis, or security review and is cross-referenced to its original source. Items are categorized by priority and complexity to guide future planning.

---

## Reliability Improvements

These items address correctness, security, and resilience gaps identified during the hardening initiative. They are not new features but rather improvements required for robust production operation.

### SEC-01: DNS Rebinding SSRF Mitigation

**Priority:** High
**Complexity:** Medium
**Category:** Security

**Source:** REQUIREMENTS.md SEC-01, PITFALLS.md Pitfall 3, FEATURES.md (HTTP Invoker Reliability Improvements)

**Current state:** The URL validator in `url_validator.rs` resolves DNS and checks for private IPs at validation time, but reqwest re-resolves the hostname at request time when `send()` is called. This creates a time-of-check-to-time-of-use (TOCTOU) gap: an attacker-controlled DNS server can return a public IP during validation, then a private IP (e.g., 169.254.169.254 for cloud metadata) when reqwest resolves the hostname again for the actual connection. This is the classic DNS rebinding attack and a well-documented SSRF bypass (OWASP).

**Gap:** Validation and connection perform two separate DNS lookups with no guarantee of consistency between them.

**Recommended approach:**
- Use `reqwest::Client::builder().resolve()` to pin the validated IP address, forcing the connection to use the IP that passed validation rather than re-resolving.
- Alternatively, implement a custom `reqwest::dns::Resolve` trait that checks the resolved IP against the private IP blocklist at connect time (not just at validation time).
- As defense-in-depth, document that operators should configure outbound firewall rules blocking RFC1918/link-local/metadata IPs from the engine process.

---

### SEC-02: Request/Response Body Size Limits

**Priority:** High
**Complexity:** Low
**Category:** Security

**Source:** REQUIREMENTS.md SEC-02, FEATURES.md (HTTP Invoker Reliability Improvements)

**Current state:** The HTTP invoker clones the request body (`body.clone()`) for HMAC signing with no size guard. An attacker or misconfigured caller could submit a multi-megabyte payload, doubling memory consumption during signing. On the response side, `reqwest::Response::bytes()` reads the entire response body into memory with no limit, allowing a malicious endpoint to return a multi-gigabyte response and cause out-of-memory conditions.

**Gap:** No configurable `max_body_size` check before serialization or before reading response bodies.

**Recommended approach:**
- Add a configurable `max_request_body_size` (e.g., 1MB default) checked before HMAC signing and body serialization.
- Add a configurable `max_response_body_size` limit on response body reads, using streaming reads with a byte counter rather than `bytes()`.
- Return clear error messages when limits are exceeded (not panics or silent truncation).

---

### SEC-03: HMAC Replay Protection Guidance

**Priority:** Low
**Complexity:** Low
**Category:** Security

**Source:** REQUIREMENTS.md SEC-03, PITFALLS.md Pitfall 6

**Current state:** The webhook delivery system signs requests with HMAC-SHA256 using `timestamp:base64(body)` as the signed payload. The `x-iii-Timestamp` header is included in outgoing requests. However, there is no receiver-side guidance for validating that the timestamp is recent, meaning a captured webhook request can be replayed indefinitely. The iii engine is the webhook sender, so replay protection is technically the receiver's responsibility, but the engine should provide guidance.

**Gap:** No documentation guiding webhook receivers to validate timestamp freshness. No nonce or unique request ID in the signed payload.

**Recommended approach:**
- Document that webhook receivers MUST validate the `x-iii-Timestamp` header and reject requests older than a configurable window (e.g., 5 minutes).
- Consider adding a unique request ID (nonce) to the signed payload to prevent exact replay.
- Provide example receiver-side verification code in documentation.

---

### REL-01: Circuit Breaker for HTTP Invocations

**Priority:** Medium
**Complexity:** High
**Category:** Resilience

**Source:** REQUIREMENTS.md REL-01, FEATURES.md (Queue Reliability Improvements -- Backlog Only)

**Current state:** No circuit breaker exists for HTTP invocations. If a downstream endpoint is consistently failing (returning 5xx, timing out, or refusing connections), the engine continues sending requests at full rate. This wastes resources, increases latency for callers, and can overwhelm recovering endpoints.

**Gap:** No per-endpoint state tracking, no failure threshold configuration, no half-open probing to detect recovery.

**Recommended approach:**
- Implement per-endpoint circuit breaker state tracking (closed, open, half-open).
- Configure failure threshold (e.g., 5 failures in 60 seconds triggers open state).
- In open state, fail fast with a clear error rather than attempting the request.
- Periodically probe with a single request (half-open state) to detect endpoint recovery.
- Expose circuit breaker state via metrics for operational visibility.

---

### REL-02: Consumer Crash Recovery

**Priority:** Medium
**Complexity:** Medium
**Category:** Resilience

**Source:** REQUIREMENTS.md REL-02, PITFALLS.md

**Current state:** If a spawned consumer task panics, the queue goes silent -- no further messages are processed for that queue. Handler panics within the consumer loop are now caught (per Phase 4 FIX using `catch_unwind + AssertUnwindSafe`), but if the consumer task itself crashes (e.g., due to a Tokio runtime error or an unrecoverable state), there is no supervisor or restart logic.

**Gap:** No supervision of consumer tasks. A crashed consumer is not detected or restarted.

**Recommended approach:**
- Implement supervisor/restart logic for consumer tasks that detects task termination and respawns.
- Log consumer death events with full context (queue name, error, last processed message).
- Add a configurable restart backoff to prevent tight restart loops on persistent failures.
- Consider a health check mechanism that monitors consumer liveness.

---

### REL-03: Message Deduplication

**Priority:** Medium
**Complexity:** Medium
**Category:** Resilience

**Source:** REQUIREMENTS.md REL-03, FEATURES.md (Queue Reliability Improvements -- Backlog Only)

**Current state:** The `message_id` field exists on `QueueMessage` but no deduplication logic is implemented. At-least-once delivery semantics mean that during retries, the same message can be delivered multiple times. Consumers must handle duplicates themselves with no framework support.

**Gap:** No idempotency window for recently-processed `message_id` values.

**Recommended approach:**
- Implement a configurable deduplication window (e.g., 5 minutes) that tracks recently-processed `message_id` values.
- Before delivering a message, check if its `message_id` was already processed within the window.
- If duplicate, acknowledge the message silently rather than redelivering.
- Storage for the dedup window can use the existing KV store for the builtin adapter or a shared cache for other adapters.

---

### REL-04: Redis Adapter E2E Test Parity

**Priority:** Low
**Complexity:** Medium
**Category:** Testing

**Source:** REQUIREMENTS.md REL-04

**Current state:** The Redis adapter exists in the codebase but has no E2E test coverage. It was explicitly out of scope for the current hardening initiative, which focused on the builtin and RabbitMQ adapters. The adapter's reliability characteristics are unknown and unverified.

**Gap:** No E2E tests for Redis adapter. No verification that happy path, retry, DLQ, or FIFO behaviors work correctly.

**Recommended approach:**
- Mirror the builtin and RabbitMQ E2E test patterns for the Redis adapter (happy path, retry/backoff, DLQ exhaustion, FIFO ordering).
- Use testcontainers with a Redis container for test isolation.
- Address any Redis-specific behavior differences (e.g., Lua scripting for atomic operations, different TTL semantics).

---

### Auth Env Var Cleanup Beyond FIX-03 Scope

**Priority:** Low
**Complexity:** Low
**Category:** Code Quality

**Source:** PITFALLS.md Pitfall 7, 10-RESEARCH.md Open Question 3

**Current state:** FIX-03 addressed the `unsafe { std::env::set_var }` pattern in `engine/src/invocation/auth.rs` by migrating to the `temp-env` crate. However, the same unsafe pattern also exists in `engine/src/invocation/http_functions/mod.rs` (line 486) and `engine/src/observability/mod.rs` (line 1682). The observability module has a hand-rolled `with_env_var` helper that lacks the mutex synchronization provided by `temp-env`.

**Gap:** Two additional test modules still use the unsafe env var pattern.

**Recommended approach:**
- Apply the same `temp-env` migration pattern used in FIX-03 to the remaining test modules.
- Replace the hand-rolled `with_env_var` in observability/mod.rs with `temp_env::with_var`.
- Remove `#[serial]` attributes from migrated tests where env var isolation was the only reason for serialization.

---

## Feature Additions

These items represent new capabilities that do not exist today. They are distinct from reliability improvements in that they add functionality rather than fixing correctness or security gaps.

### FEAT-01: Priority Queues

**Priority:** Medium
**Complexity:** High
**Category:** Queue

**Source:** REQUIREMENTS.md FEAT-01, FEATURES.md (Queue Reliability Improvements -- Backlog Only)

**Current state:** No priority queue support exists. All messages are processed in FIFO (for FIFO queues) or standard order (for standard queues). There is no mechanism to mark a message as higher priority or to process urgent messages before others.

**Value proposition:** Some messages are inherently more urgent than others (e.g., user-facing actions vs. batch processing). Priority queues allow operators to ensure critical messages are processed first without needing separate queue infrastructure.

**Dependencies:**
- New queue type or configuration option alongside Standard and FIFO
- Adapter-level changes across builtin (priority-sorted data structure) and RabbitMQ (priority queue plugin or multiple queues with consumer priority)
- SDK support for specifying priority at enqueue time

---

### Scheduled/Delayed Message Delivery API

**Priority:** Low
**Complexity:** Medium
**Category:** Queue

**Source:** FEATURES.md (Queue Reliability Improvements -- Backlog Only)

**Current state:** The builtin adapter has an internal delayed-to-waiting mechanism used for retry backoff (messages are placed in a delayed sorted set with a future timestamp and moved to the waiting queue when ready). However, there is no user-facing API to enqueue a message with a scheduled delivery time (e.g., "process this in 5 minutes").

**Value proposition:** Delayed message delivery is a common pattern for scheduled tasks, reminder systems, and rate-limited processing. Users currently have to implement their own scheduling layer outside the queue.

**Dependencies:**
- API endpoint or SDK method accepting a `deliver_after` or `scheduled_at` parameter
- Extension of the existing delayed queue mechanism to support user-specified delays (not just retry backoff)
- SDK support in Node and Python SDKs

---

### FEAT-02: DLQ Redrive API with Filtering

**Priority:** Medium
**Complexity:** Medium
**Category:** Queue

**Source:** REQUIREMENTS.md FEAT-02, FEATURES.md (Queue Reliability Improvements -- Backlog Only)

**Current state:** The `redrive_dlq` function exists and can move messages from the DLQ back to the main queue. However, it moves ALL messages indiscriminately. DLQ message inspection (viewing payload, error reason, and attempt count) was implemented in Phase 3, so operators can now see what is in the DLQ, but they cannot selectively redrive only the messages they want.

**Value proposition:** Operators need to selectively redrive messages matching certain criteria (e.g., only messages that failed due to a transient error, not messages that failed due to a permanent bug). Moving all DLQ messages back risks re-poisoning the queue with messages that will fail again.

**Dependencies:**
- DLQ message inspection (completed in Phase 3)
- Filtering criteria API (by error type, by timestamp range, by payload content)
- Selective redrive implementation that preserves unmatched messages in the DLQ

---

### FEAT-03: HTTP Invoker Retry Policies

**Priority:** Medium
**Complexity:** High
**Category:** HTTP Invoker

**Source:** REQUIREMENTS.md FEAT-03, FEATURES.md (Anti-Features table -- explicitly out of scope for hardening)

**Current state:** No automatic retry exists for HTTP invocation failures. If a target endpoint returns a transient error (503 Service Unavailable, 429 Too Many Requests) or a network timeout occurs, the invocation fails immediately and the error is returned to the caller.

**Value proposition:** Transient failures are common in distributed systems. Automatic retry with configurable backoff significantly improves reliability without requiring callers to implement their own retry logic.

**Dependencies:**
- Configurable retry count per endpoint or per invocation
- Backoff strategy configuration (fixed, exponential, with jitter)
- DLQ or error reporting for invocations that exhaust all retries
- Integration with circuit breaker (REL-01) to avoid retrying against open circuits

---

### Redis Adapter Hardening

**Priority:** Low
**Complexity:** Medium
**Category:** Adapter

**Source:** FEATURES.md (Anti-Features table), PROJECT.md Out of Scope

**Current state:** The Redis adapter exists in the codebase but is not actively used or prioritized. It has no E2E test coverage (see REL-04) and its reliability characteristics are unverified. The hardening initiative focused on builtin and RabbitMQ adapters per the project decision to scope Redis out.

**Value proposition:** Users choosing Redis as their queue backend should receive the same reliability guarantees (retry, DLQ, FIFO ordering, backoff) as builtin and RabbitMQ users. Redis is a popular infrastructure choice and supporting it well expands the engine's applicability.

**Dependencies:**
- REL-04 (E2E test parity) must be completed first to establish a baseline
- May require Redis-specific implementations (Lua scripts for atomic operations, Redis Streams for ordering guarantees)
- Performance characterization under load

---

### Queue Depth Metrics and Alerting

**Priority:** Low
**Complexity:** Medium
**Category:** Observability

**Source:** FEATURES.md (Queue Reliability Improvements -- Backlog Only)

**Current state:** OpenTelemetry tracing and metrics infrastructure exists in the engine. Queue operations emit spans for tracing. However, queue depth (number of messages waiting, number in delayed state, number in DLQ) is not instrumented as a metric. Operators have no visibility into whether queues are backing up.

**Value proposition:** Queue depth is the most important operational metric for a queue system. Without it, operators cannot detect backlogs, capacity issues, or stuck consumers until users report problems.

**Dependencies:**
- OpenTelemetry metric integration (gauge metrics for queue depth per queue name)
- Periodic sampling of queue state or event-driven counter updates
- Dashboard or alerting integration guidance

---

### Message TTL / Expiration

**Priority:** Low
**Complexity:** Medium
**Category:** Queue

**Source:** FEATURES.md (Queue Reliability Improvements -- Backlog Only)

**Current state:** No per-message TTL support exists. Messages remain in the queue indefinitely until they are processed, retried to exhaustion and DLQ'd, or the queue is cleared. Stale messages (e.g., from a batch that was already reprocessed) consume resources and can cause confusion when processed long after they were relevant.

**Value proposition:** Message TTL prevents stale messages from being processed. This is especially important for time-sensitive operations where processing an old message would produce incorrect results or trigger duplicate actions.

**Dependencies:**
- Per-message metadata field for expiration timestamp
- Adapter-level support to check TTL before delivery (builtin: check during poll, RabbitMQ: use per-message `expiration` property)
- Decision on what happens to expired messages (discard silently, move to DLQ, emit metric)

---

### Webhook Delivery Retry with Exponential Backoff

**Priority:** Medium
**Complexity:** High
**Category:** HTTP Invoker

**Source:** FEATURES.md (HTTP Invoker Reliability Improvements -- Backlog Only)

**Current state:** Failed webhook deliveries are not automatically retried. If the target endpoint is temporarily unavailable, the webhook is lost. This is a significant gap for webhook consumers who depend on reliable delivery.

**Value proposition:** Reliable webhook delivery is a baseline expectation for webhook providers. Most webhook systems (Stripe, GitHub, Twilio) retry with exponential backoff. Without retry, consumers must implement their own polling or reconciliation mechanisms to detect missed webhooks.

**Dependencies:**
- Async retry loop with configurable retry count and backoff strategy
- DLQ or persistent storage for webhook deliveries that exhaust all retries
- Backoff configuration (initial delay, multiplier, max delay, jitter)
- Integration with circuit breaker (REL-01) for endpoints that are consistently failing

---

## References

- **`.planning/REQUIREMENTS.md`** -- Full requirement definitions for all SEC, REL, FEAT, and other requirement IDs referenced in this document.
- **`.planning/research/FEATURES.md`** -- Detailed feature landscape analysis including table stakes, differentiators, backlog-only items, and anti-features with rationale.
- **`.planning/research/PITFALLS.md`** -- Technical pitfalls research including DNS rebinding (Pitfall 3), HMAC replay (Pitfall 6), and auth env var contamination (Pitfall 7).
- **`.planning/PROJECT.md`** -- Project scope, constraints, and out-of-scope decisions.

## Notes

- Priority and complexity estimates are based on initial research during the hardening initiative and are subject to refinement during future planning.
- Items are listed in approximate priority order within each section, but dependencies may dictate a different implementation sequence.
- Cross-references use requirement IDs (SEC-01, REL-01, FEAT-01, etc.) that are defined in `.planning/REQUIREMENTS.md`.
