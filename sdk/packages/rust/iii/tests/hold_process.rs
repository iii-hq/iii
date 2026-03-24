use std::sync::Arc;
use std::time::Duration;

use iii_sdk::{III, InitOptions, register_worker};

#[tokio::test]
async fn hold_async_resolves_on_shutdown() {
    // register_worker calls connect() internally, setting running=true
    let iii = register_worker("ws://127.0.0.1:1", InitOptions::default());

    let iii_clone = iii.clone();
    let resolved = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let resolved_clone = resolved.clone();

    let handle = tokio::spawn(async move {
        iii_clone.hold_async().await;
        resolved_clone.store(true, std::sync::atomic::Ordering::SeqCst);
    });

    // Give the spawned task time to start waiting
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        !resolved.load(std::sync::atomic::Ordering::SeqCst),
        "hold_async should still be waiting"
    );

    iii.shutdown_async().await;

    // Wait for the spawned task to finish
    tokio::time::timeout(Duration::from_secs(2), handle)
        .await
        .expect("hold_async should have resolved within timeout")
        .expect("task should not panic");

    assert!(
        resolved.load(std::sync::atomic::Ordering::SeqCst),
        "hold_async should have resolved after shutdown"
    );
}

#[tokio::test]
async fn hold_async_returns_immediately_when_not_running() {
    // III::new() does NOT call connect(), so running is false
    let iii = III::new("ws://127.0.0.1:1");

    // hold_async should return immediately since running is false
    let result = tokio::time::timeout(Duration::from_millis(200), iii.hold_async()).await;
    assert!(
        result.is_ok(),
        "hold_async should return immediately when not running"
    );
}
