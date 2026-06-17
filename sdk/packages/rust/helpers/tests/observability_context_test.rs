use iii_helpers::observability::{current_span_id, current_trace_id};

#[test]
fn outside_span_returns_none() {
    assert_eq!(current_span_id(), None);
    assert_eq!(current_trace_id(), None);
}
