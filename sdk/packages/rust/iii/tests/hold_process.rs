use std::time::Duration;

use iii_sdk::{IIIConnectionState, InitOptions, register_worker};

#[test]
fn shutdown_stops_connection_thread() {
    // register_worker spawns a non-daemon connection thread that
    // keeps the process alive. shutdown() joins it.
    let iii = register_worker("ws://127.0.0.1:1", InitOptions::default());

    // Give the connection thread time to start
    std::thread::sleep(Duration::from_millis(50));

    // shutdown() should signal and join the thread
    iii.shutdown();

    assert_eq!(iii.get_connection_state(), IIIConnectionState::Disconnected);
}

#[test]
fn shutdown_completes_quickly() {
    let iii = register_worker("ws://127.0.0.1:1", InitOptions::default());

    let start = std::time::Instant::now();
    iii.shutdown();
    let elapsed = start.elapsed();

    assert!(
        elapsed < Duration::from_secs(5),
        "shutdown took too long: {:?}",
        elapsed,
    );
}
