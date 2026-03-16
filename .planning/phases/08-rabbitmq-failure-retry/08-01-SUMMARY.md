---
phase: 08-rabbitmq-failure-retry
plan: 01
subsystem: testing
tags: [rabbitmq, retry, dlq, amqp, lapin, backoff, dead-letter, e2e]

# Dependency graph
requires:
  - phase: 07-rabbitmq-happy-path-topology
    provides: RabbitMQ testcontainer infrastructure, shared container via get_rabbitmq(), topology verification patterns
  - phase: 03-builtin-queue-failure-retry
    provides: Failure/retry test patterns (register_failing_function_with_timestamps, dlq_count)
provides:
  - Three E2E tests covering RabbitMQ failure/retry paths (flat backoff timing, DLQ content verification, max_retries=0 boundary)
  - rabbitmq_queue_config_custom helper for per-test retry configuration
  - Direct AMQP basic_get pattern for DLQ content inspection
affects: []

# Tech tracking
tech-stack:
  added: []
  patterns: [direct AMQP basic_get for DLQ content inspection, custom queue config builder for boundary testing]

key-files:
  created: []
  modified:
    - engine/tests/common/rabbitmq_helpers.rs
    - engine/tests/rabbitmq_queue_integration.rs

key-decisions:
  - "Used direct AMQP basic_get for DLQ content inspection instead of adapter dlq_messages (returns empty for RabbitMQ)"
  - "Flat backoff timing uses 400ms-3000ms tolerance band for CI stability with 500ms TTL"

patterns-established:
  - "Direct AMQP basic_get for RabbitMQ DLQ content verification (x-death headers, raw payload)"
  - "rabbitmq_queue_config_custom for per-test max_retries and backoff_ms configuration"

requirements-completed: [QRMQ-02, QRMQ-03, QRMQ-05]

# Metrics
duration: 4min
completed: 2026-03-16
---

# Phase 8 Plan 1: RabbitMQ Failure & Retry Summary

**Three E2E tests verifying RabbitMQ flat TTL backoff timing, DLQ exhaustion with raw payload and x-death header content via direct AMQP basic_get, and max_retries=0 boundary behavior**

## Performance

- **Duration:** 4 min
- **Started:** 2026-03-16T18:36:29Z
- **Completed:** 2026-03-16T18:40:29Z
- **Tasks:** 2
- **Files modified:** 2

## Accomplishments
- Verified RabbitMQ flat (constant) backoff: 4 handler invocations with all 3 retry intervals within 400ms-3000ms at 500ms TTL
- Verified DLQ exhaustion routes raw original payload to DLQ with RabbitMQ x-death headers proving dead-letter mechanism
- Verified max_retries=0 calls handler exactly once, routes directly to DLQ, and produces no further retries
- Added rabbitmq_queue_config_custom helper for focused per-test retry configuration

## Task Commits

Each task was committed atomically:

1. **Task 1: Add custom config helper and flat backoff timing test (QRMQ-02)** - `29deaf0c` (test)
2. **Task 2: Add DLQ content verification (QRMQ-03) and max_retries=0 boundary (QRMQ-05) tests** - `ed634256` (test)

## Files Created/Modified
- `engine/tests/common/rabbitmq_helpers.rs` - Added rabbitmq_queue_config_custom helper with per-test max_retries and backoff_ms
- `engine/tests/rabbitmq_queue_integration.rs` - Added 3 new E2E tests (rmq_retry_backoff_timing_is_flat, rmq_dlq_exhaustion_with_content_verification, rmq_max_retries_zero_sends_directly_to_dlq)

## Decisions Made
- Used direct AMQP basic_get for DLQ content inspection because adapter's dlq_messages returns empty for RabbitMQ (default trait impl)
- Used 400ms-3000ms tolerance band for flat backoff timing assertions to account for container scheduling and CI variability
- Did not assert x-attempt header for max_retries=0 test since message goes directly to DLQ without retry exchange republication

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- All RabbitMQ failure/retry E2E coverage complete (flat backoff, DLQ content, zero-retry boundary)
- Full suite: 12 RabbitMQ integration tests passing (9 pre-existing + 3 new)
- Ready for next phase

## Self-Check: PASSED

All files exist, all commits verified.

---
*Phase: 08-rabbitmq-failure-retry*
*Completed: 2026-03-16*
