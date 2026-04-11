use std::sync::Arc;

use iii::engine::Engine;
use iii::function::{Function, FunctionResult};
use iii::workers::reload::WorkerRegistrations;

fn make_dummy_function(id: &str) -> Function {
    Function {
        handler: Arc::new(|_invocation_id, _input, _session| {
            Box::pin(async { FunctionResult::Success(None) })
        }),
        _function_id: id.to_string(),
        _description: None,
        request_format: None,
        response_format: None,
        metadata: None,
    }
}

#[test]
fn scope_begin_end_lifecycle() {
    let engine = Arc::new(Engine::new());
    engine.begin_worker_scope("test::Worker");
    let regs = engine.end_worker_scope();
    assert!(regs.function_ids.is_empty());
}

#[test]
fn remove_worker_registrations_clears_functions() {
    let engine = Arc::new(Engine::new());
    // Manually seed a function, then construct WorkerRegistrations by hand
    // (Task 1 has no scope interception yet; that's Task 2).
    let function_id = "test::Worker::handler".to_string();
    engine
        .functions
        .register_function(function_id.clone(), make_dummy_function(&function_id));
    assert!(engine.functions.get(&function_id).is_some());

    let regs = WorkerRegistrations {
        function_ids: vec![function_id.clone()],
    };
    engine.remove_worker_registrations(&regs);

    assert!(engine.functions.get(&function_id).is_none());
}
