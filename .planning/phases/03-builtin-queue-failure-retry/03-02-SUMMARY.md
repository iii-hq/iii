---
phase: 03-builtin-queue-failure-retry
plan: 02
subsystem: queue
tags: [backoff, dlq, retry, exponential-backoff, e2e-test, rust]

# Dependency graph
requires:
  - phase: 03-builtin-queue-failure-retry
    plan: 01
    provides: "Saturating backoff fix, DLQ content inspection chain (dlq_messages helper)"
  - phase: 01-test-infrastructure
    provides: "Test helpers (queue_helpers.rs), Engine test patterns"
provides:
  - "E2E test proving exponential backoff timing with ratio verification"
  - "E2E test proving DLQ exhaustion preserves payload, error, attempt count"
  - "E2E test proving max_retries=0 boundary sends directly to DLQ"
  - "register_failing_function_with_timestamps helper for timing-based failure tests"
affects: [07-rabbitmq-queue-failure-retry]

# Tech tracking
tech-stack:
  added: []
  patterns: ["real wall-clock time for function queue retry tests (nack path uses SystemTime)", "start_paused only for zero-retry DLQ tests"]

key-files:
  created:
    - engine/tests/queue_e2e_failure_retry.rs
  modified:
    - engine/tests/common/queue_helpers.rs

key-decisions:
  - "Used real wall-clock time (not start_paused) for backoff timing tests because function queue nack path uses SystemTime::now() for delayed-to-waiting transition"
  - "Used std::time::Instant (not tokio::time::Instant) in timestamp helper since function queue consumer operates on real time"
  - "Used max_retries=4 for QBLT-02 to get 4 invocations and 3 measurable backoff intervals"

patterns-established:
  - "Function queue consumer always uses nack->delayed->re-poll path, never FIFO inline retry"
  - "start_paused is safe for zero-retry tests (no delayed jobs) but not for retry tests (SystemTime conflict)"

requirements-completed: [QBLT-02, QBLT-03, QBLT-05]

# Metrics
duration: 7min
completed: 2026-03-16
---

# Phase 03 Plan 02: E2E Failure/Retry Tests Summary

**Three E2E tests verifying exponential backoff timing with ratio assertions, DLQ exhaustion with full content verification, and max_retries=0 boundary behavior**

## Performance

- **Duration:** 7 min
- **Started:** 2026-03-16T14:59:06Z
- **Completed:** 2026-03-16T15:07:00Z
- **Tasks:** 2
- **Files modified:** 2

## Accomplishments
- Created 3 E2E tests covering QBLT-02, QBLT-03, QBLT-05 requirements
- Verified exponential backoff timing with both threshold assertions and ratio checks (each interval ~2x previous)
- Verified DLQ message contains original payload, error string, attempt count, max_attempts, and failed_at timestamp
- Verified max_retries=0 causes exactly 1 handler invocation then DLQ with no retries
- All 1243 library tests + 15 integration/E2E tests pass

## Task Commits

Each task was committed atomically:

1. **Task 1: Add timestamp-recording failing handler helper** - `83080ce8` (feat)
2. **Task 2: Create queue_e2e_failure_retry.rs with 3 E2E tests** - `ec467269` (test)

## Files Created/Modified
- `engine/tests/queue_e2e_failure_retry.rs` - 3 E2E tests: retry_backoff_timing_is_exponential, dlq_exhaustion_preserves_payload_and_metadata, max_retries_zero_sends_directly_to_dlq
- `engine/tests/common/queue_helpers.rs` - Added register_failing_function_with_timestamps helper using std::time::Instant

## Decisions Made
- Used real wall-clock time for QBLT-02 and QBLT-03 because the function queue consumer nack path uses SystemTime::now() for the delayed-to-waiting transition, which does not auto-advance under start_paused
- Used std::time::Instant instead of tokio::time::Instant in the timestamps helper for the same reason
- Used max_retries=4 (not 3 as plan suggested) to get exactly 4 invocations with 3 measurable backoff intervals, since max_retries maps directly to max_attempts
- Used standard queue type (not FIFO) for all tests since the function queue consumer path does not use the FIFO inline retry

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Corrected start_paused assumption for retry tests**
- **Found during:** Task 2 (test creation)
- **Issue:** Plan assumed FIFO queue config would use inline retry path with tokio::time::sleep (compatible with start_paused). In reality, the function queue consumer always uses the nack -> delayed sorted set -> move_delayed_to_waiting path, which uses SystemTime::now(). Under start_paused, real time barely advances, so delayed jobs never become ready.
- **Fix:** Removed start_paused from QBLT-02 and QBLT-03 tests, used small backoff values (50ms) with real wall-clock time and generous tolerance assertions. Changed timestamp type from tokio::time::Instant to std::time::Instant.
- **Files modified:** engine/tests/queue_e2e_failure_retry.rs, engine/tests/common/queue_helpers.rs
- **Verification:** All 3 tests pass consistently
- **Committed in:** ec467269

**2. [Rule 1 - Bug] Corrected max_retries config for QBLT-02**
- **Found during:** Task 2 (test creation)
- **Issue:** Plan specified max_retries=3 expecting 4 invocations (1+3). But max_retries maps directly to max_attempts, so max_retries=3 gives only 3 invocations and 2 intervals.
- **Fix:** Used max_retries=4 to get 4 invocations and 3 measurable exponential intervals (50ms, 100ms, 200ms).
- **Files modified:** engine/tests/queue_e2e_failure_retry.rs
- **Verification:** Test verifies exactly 4 calls and 3 intervals with correct exponential ratios
- **Committed in:** ec467269

---

**Total deviations:** 2 auto-fixed (2 bugs in plan assumptions)
**Impact on plan:** Both fixes were necessary for test correctness. The tests still verify the exact same requirements (QBLT-02, QBLT-03, QBLT-05) with the correct implementation approach.

## Issues Encountered
- Pre-existing compilation errors in benchmark files (protocol_serde_bench) due to missing `action` field on `Message` struct -- unrelated to our changes
- Pre-existing RabbitMQ test failures due to Docker not running -- expected in local dev

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- All Phase 3 requirements complete (FIX-01, QBLT-02, QBLT-03, QBLT-05 verified)
- E2E failure/retry test patterns established for reuse in RabbitMQ queue tests (Phase 7)
- Key insight documented: function queue consumer always uses nack path, not FIFO inline retry

## Self-Check: PASSED

All 2 created/modified files verified present. Both task commits (83080ce8, ec467269) verified in git log. All 3 test function names and helper function confirmed in source.

---
*Phase: 03-builtin-queue-failure-retry*
*Completed: 2026-03-16*
