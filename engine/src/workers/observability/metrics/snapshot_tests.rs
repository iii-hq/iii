//! Instance-local synchronization probes; compiled out of production.
use super::*;
use std::sync::{
    Barrier,
    atomic::{AtomicBool, Ordering},
};

struct Pause {
    armed: AtomicBool,
    reached: Barrier,
    release: Barrier,
}

impl Pause {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            armed: AtomicBool::new(true),
            reached: Barrier::new(2),
            release: Barrier::new(2),
        })
    }

    // No assertion/callback/allocation can unwind through a partial commit.
    fn wait(&self) {
        if self.armed.swap(false, Ordering::Relaxed) {
            self.reached.wait();
            self.release.wait();
        }
    }
}

#[derive(Default)]
pub(super) struct TestSync {
    writer: Option<Arc<Pause>>,
    reader: Option<Arc<Pause>>,
    blocked: Option<Arc<Barrier>>,
    acknowledged: AtomicBool,
}

impl TestSync {
    pub(super) fn writer_pause(&self) {
        if let Some(pause) = &self.writer {
            pause.wait();
        }
    }
    pub(super) fn reader_pause(&self) {
        if let Some(pause) = &self.reader {
            pause.wait();
        }
    }
    pub(super) fn blocked(&self) {
        if !self.acknowledged.swap(true, Ordering::Relaxed) {
            if let Some(ack) = &self.blocked {
                ack.wait();
            }
        }
    }
}

fn image(
    view: super::super::InvocationsView,
) -> (u64, u64, u64, u64, std::collections::HashMap<String, u64>) {
    (
        view.total,
        view.success,
        view.error,
        view.deferred,
        view.by_function,
    )
}

fn expected(
    total: u64,
    success: u64,
    error: u64,
    deferred: u64,
    entries: &[(&str, u64)],
) -> (u64, u64, u64, u64, std::collections::HashMap<String, u64>) {
    (
        total,
        success,
        error,
        deferred,
        entries
            .iter()
            .map(|(key, count)| ((*key).to_owned(), *count))
            .collect(),
    )
}

#[test]
fn snapshot_waits_for_each_complete_writer_transaction() {
    // Both a first insertion and an existing key use the actual commit path.
    for existing in [false, true] {
        for sdk in [false, true] {
            let pause = Pause::new();
            let ack = Arc::new(Barrier::new(2));
            let mut acc = MetricsAccumulator::default();
            if existing {
                acc.record_local_invocations("fn", 2, 1);
            }
            let before = image(acc.snapshot_invocations());
            assert_eq!(
                before,
                if existing {
                    expected(2, 1, 1, 0, &[("fn", 2)])
                } else {
                    expected(0, 0, 0, 0, &[])
                }
            );
            acc.test_sync.writer = Some(pause.clone());
            acc.test_sync.blocked = Some(ack.clone());
            let after = std::thread::scope(|scope| {
                let writer = scope.spawn(|| {
                    if sdk {
                        acc.record_local_invocations("fn", 5, 2);
                    } else {
                        acc.record_invocation("fn", InvocationOutcome::Deferred);
                    }
                });
                pause.reached.wait(); // Aggregates changed, map not yet committed.
                let reader = scope.spawn(|| image(acc.snapshot_invocations()));
                ack.wait(); // Same acquisition path positively saw WouldBlock.
                pause.release.wait();
                writer.join().unwrap(); // Completion, not merely dispatch, is asserted.
                reader.join().unwrap()
            });
            let n = if existing { 2 } else { 0 };
            let s = if existing { 1 } else { 0 };
            assert_eq!(
                after,
                if sdk {
                    expected(n + 5, s + 3, s + 2, 0, &[("fn", n + 5)])
                } else {
                    expected(n + 1, s, s, 1, &[("fn", n + 1)])
                }
            );
            assert_eq!(image(acc.snapshot_invocations()), after);
        }
    }
}

#[test]
fn writer_waits_until_snapshot_has_copied_the_map() {
    let pause = Pause::new();
    let ack = Arc::new(Barrier::new(2));
    let mut acc = MetricsAccumulator::default();
    acc.record_invocation("old", InvocationOutcome::Success);
    let before = image(acc.snapshot_invocations());
    acc.test_sync.reader = Some(pause.clone());
    acc.test_sync.blocked = Some(ack.clone());
    let captured = std::thread::scope(|scope| {
        let reader = scope.spawn(|| image(acc.snapshot_invocations()));
        pause.reached.wait(); // Four copied; map clone still pending.
        let writer = scope.spawn(|| acc.record_invocation("new", InvocationOutcome::Error));
        ack.wait();
        pause.release.wait();
        let captured = reader.join().unwrap();
        writer.join().unwrap();
        captured
    });
    assert_eq!(captured, before);
    assert_eq!(
        image(acc.snapshot_invocations()),
        expected(2, 1, 1, 0, &[("old", 1), ("new", 1)])
    );
}

#[test]
fn legacy_split_read_schedule_is_torn_even_with_complete_writes() {
    // Deterministic reproduction of the old reader schedule, not a claim about
    // the unknown Run15 interleaving. A barrier puts a whole write between reads.
    let acc = MetricsAccumulator::default();
    let barrier = Barrier::new(2);
    std::thread::scope(|scope| {
        let writer = scope.spawn(|| {
            barrier.wait();
            acc.record_invocation("fn", InvocationOutcome::Success);
            barrier.wait();
        });
        let old = acc.invocation_counts();
        barrier.wait();
        barrier.wait();
        assert_eq!((old.total, acc.get_by_function()["fn"]), (0, 1));
        writer.join().unwrap();
    });
    assert_eq!(
        image(acc.snapshot_invocations()),
        expected(1, 1, 0, 0, &[("fn", 1)])
    );
}

#[test]
fn local_rows_preserve_clamp_zero_and_mixed_outcomes() {
    let acc = MetricsAccumulator::default();
    acc.record_local_invocations("mixed", 5, 2);
    acc.record_local_invocations("clamped", 3, 9);
    acc.record_local_invocations("zero", 0, u64::MAX);
    assert_eq!(
        image(acc.snapshot_invocations()),
        expected(8, 3, 5, 0, &[("mixed", 5), ("clamped", 3), ("zero", 0)])
    );
    assert!(acc.first_user_success_fn.get().is_none());
    assert!(acc.first_user_failure_fn.get().is_none());
}

#[test]
fn owned_reads_and_iterator_do_not_retain_locks_or_live_values() {
    let acc = MetricsAccumulator::default();
    acc.record_invocation("fn", InvocationOutcome::Success);
    let snapshot = acc.snapshot_invocations();
    let map = acc.get_by_function();
    let mut iter = acc.iter_function_counts();
    assert!(acc.invocations.try_lock().is_ok());
    acc.record_invocation("fn", InvocationOutcome::Error);
    assert_eq!(acc.get_function_count("fn"), Some(2));
    assert_eq!(iter.next(), Some(("fn".to_owned(), 1)));
    drop(iter);
    assert!(acc.invocations.try_lock().is_ok());
    assert_eq!(map["fn"], 1);
    assert_eq!(image(snapshot), expected(1, 1, 0, 0, &[("fn", 1)]));
}

#[test]
fn mixed_overflow_retains_wrapping_dispatch_and_saturating_sdk_map() {
    let acc = MetricsAccumulator::default();
    acc.record_local_invocations("fn", u64::MAX, 0);
    acc.record_local_invocations("fn", 1, 0);
    assert_eq!(
        image(acc.snapshot_invocations()),
        expected(u64::MAX, u64::MAX, 0, 0, &[("fn", u64::MAX)])
    );
    acc.record_invocation("fn", InvocationOutcome::Success);
    assert_eq!(
        image(acc.snapshot_invocations()),
        expected(0, 0, 0, 0, &[("fn", u64::MAX)])
    );
    acc.record_local_invocations("error", u64::MAX, u64::MAX);
    acc.record_invocation("error", InvocationOutcome::Error);
    assert_eq!(
        image(acc.snapshot_invocations()),
        expected(0, 0, 0, 0, &[("fn", u64::MAX), ("error", u64::MAX)])
    );
    // Boundary fixture is complete in wrapping arithmetic; no partial recovery.
    {
        let mut state = acc.invocation_state();
        *state = InvocationState {
            counts: InvocationCounts {
                total: u64::MAX,
                deferred: u64::MAX,
                ..Default::default()
            },
            by_function: [("deferred".to_owned(), u64::MAX)].into(),
        };
    }
    acc.record_invocation("deferred", InvocationOutcome::Deferred);
    assert_eq!(
        image(acc.snapshot_invocations()),
        expected(0, 0, 0, 0, &[("deferred", u64::MAX)])
    );
}

#[test]
fn poison_before_or_after_complete_commit_preserves_state() {
    for committed in [false, true] {
        let acc = MetricsAccumulator::default();
        if committed {
            acc.record_invocation("fn", InvocationOutcome::Success);
        }
        let before = image(acc.snapshot_invocations());
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _state = acc.invocation_state();
            // Never inject a panic inside the numeric transaction.
            panic!("poison with a complete invariant");
        }));
        assert!(result.is_err());
        assert!(acc.invocations.is_poisoned());
        assert_eq!(image(acc.snapshot_invocations()), before);
        acc.record_invocation("fn", InvocationOutcome::Error);
        let n = u64::from(committed);
        assert_eq!(
            image(acc.snapshot_invocations()),
            expected(n + 1, n, 1, 0, &[("fn", n + 1)])
        );
    }
}
