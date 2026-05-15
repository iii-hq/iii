//! Verify the engine's `iii worker init` passthrough routes correctly. We
//! don't exercise the full binary-download flow (would require network or a
//! prepared cache); we just check that the engine accepts `worker init` as a
//! valid passthrough and does not parse it as an unknown top-level subcommand.

use std::process::Command;

fn iii_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_iii"))
}

#[test]
fn engine_recognizes_worker_init_as_passthrough() {
    // The dispatcher will try to invoke iii-worker, which may or may not be
    // present or current in the local managed-binary cache. We accept: (a)
    // successful invocation reaching a new iii-worker, (b) old iii-worker
    // rejecting `init`, or (c) managed-binary retrieval output. Each outcome
    // proves the engine routed the subcommand to worker passthrough rather
    // than rejecting it as an unknown top-level engine arg.
    let out = iii_bin()
        .args(["worker", "init", "--help"])
        .output()
        .expect("run iii");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let combined = format!("{stdout}{stderr}");

    let reached_worker = combined.contains("--directory")
        || combined.contains("Usage: iii worker <COMMAND>")
        || combined.contains("Retrieving dependencies for worker");

    // Engine-level clap rejection would not show iii-worker's usage line.
    assert!(
        reached_worker && !combined.contains("Usage: iii <COMMAND>"),
        "engine rejected `worker init` instead of passing through; combined output: {combined}"
    );
}
