---
phase: 4
slug: builtin-queue-concurrency-resilience
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-16
---

# Phase 4 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo test (Rust built-in) |
| **Config file** | `engine/Cargo.toml` |
| **Quick run command** | `cargo test -p iii --test queue_e2e_concurrency_resilience 2>&1 \| tail -10` |
| **Full suite command** | `cargo test -p iii --all-targets 2>&1 \| tail -20` |
| **Estimated runtime** | ~20 seconds |

---

## Sampling Rate

- **After every task commit:** Run quick command
- **After every plan wave:** Run full suite
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 04-01-01 | 01 | 1 | FIX-02, QBLT-06 | unit | `cargo check -p iii 2>&1 \| tail -10` | N/A (source fix) | pending |
| 04-01-02 | 01 | 1 | QBLT-06, FIX-02 | integration | `cargo test -p iii --test queue_e2e_concurrency_resilience -- --nocapture 2>&1 \| tail -10` | W0 | pending |
| 04-02-01 | 02 | 2 | QBLT-04 | unit | `cargo check -p iii --tests 2>&1 \| tail -10` | N/A (helper) | pending |
| 04-02-02 | 02 | 2 | QBLT-04 | integration | `cargo test -p iii --test queue_e2e_concurrency_resilience fifo_multi_group -- --nocapture 2>&1 \| tail -10` | W0 | pending |

---

## Wave 0 Requirements

- [ ] `engine/tests/queue_e2e_concurrency_resilience.rs` — E2E test file for concurrency/resilience scenarios

---

## Manual-Only Verifications

*All phase behaviors have automated verification.*

---

## Validation Sign-Off

- [ ] All tasks have automated verify
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
