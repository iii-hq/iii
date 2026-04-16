//! E2E test for the quickstart template.
//!
//! Scaffolds the template once, starts the engine and both workers, then
//! uses `iii trigger` to call functions directly from the CLI.
//!
//! Run with:
//!   cargo test --test e2e_quickstart -- --ignored --nocapture
//!
//! Requires:
//!   - `iii` binary on PATH (or III_BIN env var)
//!   - Node.js + npm (for caller-worker)
//!   - Python 3 (for math-worker)

mod e2e_harness;

use std::time::Duration;

#[tokio::test]
#[ignore]
async fn quickstart() {
    let mut scenario = e2e_harness::Scenario::builder("quickstart", "iii")
        .build()
        .await;

    scenario.start_engine().await;
    scenario.start_workers().await;

    eprintln!("[test] waiting for workers to register...");
    tokio::time::sleep(Duration::from_secs(10)).await;

    // 1. Call the Python worker directly
    eprintln!("[test] iii trigger math::add");
    let stdout = scenario
        .run_iii_with_output(&[
            "trigger",
            "--function-id=math::add",
            "--payload={\"a\": 2, \"b\": 3}",
        ])
        .await;

    eprintln!("[test] stdout: {stdout}");
    let result: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("failed to parse trigger output as JSON: {e}\nraw: {stdout}"));
    assert_eq!(result["c"], 5, "math::add should return a + b: {result}");

    // 2. Call the Node worker, which internally calls the Python worker
    eprintln!("[test] iii trigger math::add_two_numbers");
    let stdout = scenario
        .run_iii_with_output(&[
            "trigger",
            "--function-id=math::add_two_numbers",
            "--payload={\"a\": 10, \"b\": 20}",
        ])
        .await;

    eprintln!("[test] stdout: {stdout}");
    let result: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("failed to parse trigger output as JSON: {e}\nraw: {stdout}"));
    assert_eq!(
        result["c"], 30,
        "math::add_two_numbers should return a + b via cross-worker call: {result}"
    );

    scenario.shutdown().await;
}
