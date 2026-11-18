use super::*;
use crate::{
    engine::{Engine, InvocationKind},
    workers::observability::metrics::{InvocationCounts, MetricsAccumulator},
};

#[tokio::test]
async fn sdk_owner_rows_commit_mixed_clamped_and_zero_counts_only() {
    let engine = Engine::new();
    let acc = MetricsAccumulator::default();
    let owner = Uuid::new_v4();
    for name in ["local::mixed", "local::clamped", "local::zero"] {
        engine.function_owners.insert(
            ("default".into(), name.into()),
            (owner, InvocationKind::Regular),
        );
    }
    let row = |name: &str, count, errors| LocalFunctionStats {
        function_id: name.into(),
        count,
        errors,
        duration_ms_sum: 0.0,
    };
    let rows = [
        row("local::mixed", 5, 2),
        row("local::clamped", 3, 9),
        row("local::zero", 0, u64::MAX),
        row("local::not-owned", 7, 0),
    ];
    engine
        .observers
        .local_stats_with_accumulator(owner, "default", &rows, &[], &acc);
    let snapshot = acc.snapshot_invocations();
    assert_eq!(
        acc.invocation_counts(),
        InvocationCounts {
            total: 8,
            success: 3,
            error: 5,
            deferred: 0
        }
    );
    let expected: std::collections::HashMap<String, u64> = [
        ("local::mixed".into(), 5),
        ("local::clamped".into(), 3),
        ("local::zero".into(), 0),
    ]
    .into();
    assert_eq!(snapshot.by_function, expected);
    assert_eq!(acc.get_function_count("local::not-owned"), None);
    engine
        .observers
        .local_stats_with_accumulator(Uuid::new_v4(), "default", &rows, &[], &acc);
    engine
        .observers
        .local_stats_with_accumulator(owner, "other", &rows, &[], &acc);
    let after_refusals = acc.snapshot_invocations();
    assert_eq!(
        (
            after_refusals.total,
            after_refusals.success,
            after_refusals.error,
            after_refusals.deferred
        ),
        (8, 3, 5, 0)
    );
    assert_eq!(after_refusals.by_function, snapshot.by_function);
    assert!(acc.first_user_success_fn.get().is_none());
    assert!(acc.first_user_failure_fn.get().is_none());
}
