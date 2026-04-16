//! E2E test for the multi-worker-orchestration template.
//!
//! Scaffolds the template once, starts the engine and all 4 workers, then
//! hits the `/health` and `/orchestrate` endpoints.
//!
//! Run with:
//!   cargo test --test e2e_multi_worker_orchestration -- --ignored --nocapture
//!
//! Requires:
//!   - `iii` binary on PATH (or III_BIN env var)
//!   - Node.js + npm (for client and payment-worker)
//!   - Python 3 (for data-worker)
//!   - Rust/Cargo (for compute-worker)

mod e2e_harness;

use serde_json::json;
use std::time::Duration;

#[tokio::test]
#[ignore]
async fn multi_worker_orchestration() {
    let mut scenario = e2e_harness::Scenario::builder("multi-worker-orchestration", "iii")
        .build()
        .await;

    scenario.run_iii(&["worker", "add", "iii-http"]).await;
    scenario.run_iii(&["worker", "add", "iii-state"]).await;
    scenario.run_iii(&["worker", "add", "iii-cron"]).await;

    scenario.read_http_port();
    scenario.start_engine().await;
    scenario.start_workers().await;

    eprintln!("[test] waiting for HTTP readiness...");
    scenario.wait_for_http(Duration::from_secs(120)).await;
    eprintln!("[test] HTTP ready");

    // 1. Health endpoint (only needs the client worker)
    eprintln!("[test] GET /health");
    let resp = scenario.http_get("/health").await;

    let status = resp.status();
    let body: serde_json::Value = resp.json().await.unwrap();
    eprintln!(
        "[test] response status={status}, body={}",
        serde_json::to_string_pretty(&body).unwrap()
    );

    assert_eq!(status, 200, "health should return 200");
    assert_eq!(body["healthy"], true, "body: {body}");

    // 2. Orchestrate endpoint (calls all 4 workers)
    //    Retry until all workers have registered — the Rust worker needs
    //    cargo compile time on first run.
    eprintln!("[test] POST /orchestrate (with retry for worker readiness)");
    let deadline = std::time::Instant::now() + Duration::from_secs(180);
    let body: serde_json::Value = loop {
        let resp = scenario
            .http_post(
                "/orchestrate",
                json!({"data": {"message": "hello from e2e"}, "n": 42}),
            )
            .await;

        let status = resp.status();
        let b: serde_json::Value = resp.json().await.unwrap();

        let has_errors = b["errors"]
            .as_array()
            .map_or(false, |a| !a.is_empty());

        if status == 200 && !has_errors {
            eprintln!(
                "[test] response status={status}, body={}",
                serde_json::to_string_pretty(&b).unwrap()
            );
            break b;
        }

        if std::time::Instant::now() > deadline {
            eprintln!(
                "[test] response status={status}, body={}",
                serde_json::to_string_pretty(&b).unwrap()
            );
            panic!(
                "orchestrate still has errors after timeout: {}",
                b["errors"]
            );
        }

        eprintln!("[test] workers not all ready, retrying in 3s...");
        tokio::time::sleep(Duration::from_secs(3)).await;
    };

    assert_eq!(body["client"], "ok", "body: {body}");
    assert_eq!(
        body["computeWorker"]["result"], 84,
        "computeWorker: {}",
        body["computeWorker"]
    );
    assert_eq!(
        body["computeWorker"]["input"], 42,
        "computeWorker: {}",
        body["computeWorker"]
    );
    assert_eq!(
        body["dataWorker"]["source"], "data-worker",
        "dataWorker: {}",
        body["dataWorker"]
    );
    assert_eq!(
        body["externalWorker"]["body"]["message"], "Payment recorded",
        "externalWorker: {}",
        body["externalWorker"]
    );

    scenario.shutdown().await;
}
