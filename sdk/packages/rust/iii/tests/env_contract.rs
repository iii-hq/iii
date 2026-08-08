//! Engine address resolution: explicit argument, then `III_URL`, then the
//! default.
//!
//! The supervisor that spawned this process -- `iii compose`, a container
//! runtime, systemd -- sets `III_URL`, the same way it sets `III_NAMESPACE` and
//! `III_WORKER_NAME`.
//!
//! Every case runs in one test: `III_URL` is process-wide state, and cargo runs
//! tests in threads.

use iii_sdk::{DEFAULT_ENGINE_URL, engine_url_from_env};

#[test]
fn engine_url_resolution() {
    let previous = std::env::var("III_URL").ok();
    unsafe { std::env::remove_var("III_URL") };

    assert_eq!(engine_url_from_env(), DEFAULT_ENGINE_URL);
    assert_eq!(DEFAULT_ENGINE_URL, "ws://127.0.0.1:49134");

    unsafe { std::env::set_var("III_URL", "ws://engine.example:9000") };
    assert_eq!(engine_url_from_env(), "ws://engine.example:9000");

    // An empty value is not an address; fall back rather than dial "".
    unsafe { std::env::set_var("III_URL", "") };
    assert_eq!(engine_url_from_env(), DEFAULT_ENGINE_URL);

    match previous {
        Some(value) => unsafe { std::env::set_var("III_URL", value) },
        None => unsafe { std::env::remove_var("III_URL") },
    }
}
