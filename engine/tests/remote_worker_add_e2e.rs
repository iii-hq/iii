// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! End-to-end contract for where `iii worker add` lands, run against a REAL
//! engine binary plus a REAL iii-worker binary (worker-ops daemon included):
//!
//! 1. `add --host <host:port>` routes through the engine's `worker::add`
//!    trigger: the ENGINE's project config gains the worker and the CLI's
//!    own working directory is left untouched.
//! 2. `add` WITHOUT `--host` never connects to any engine — not even one
//!    listening on this machine. It is a local-file operation in the CLI's
//!    cwd: that directory's config.yaml gains the worker and the engine's
//!    config never does. (The `--host localhost` → port 49134 default-host
//!    mapping itself is a pure URL rule, pinned by unit tests in
//!    `iii-worker::cli::remote_ops`; binding the real 49134 here would
//!    collide with developer engines and parallel CI jobs, so the e2e runs
//!    on an ephemeral port.)
//!
//! The whole scenario runs inside a tempdir HOME so neither the developer's
//! `~/.iii` state nor their real engine can be seen or touched.

#![cfg(unix)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Kills the engine on EVERY exit path including assertion panics —
/// `std::process::Child` does not kill on drop, and a leaked engine keeps
/// its worker-ops daemon alive with it.
struct KillOnDrop(std::process::Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn iii_bin() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_iii"))
}

/// The iii-worker binary built into the same target dir as the engine under
/// test. `None` when it hasn't been built — the test soft-skips then, so
/// `cargo test -p iii` on a fresh checkout stays green (CI's coverage job
/// builds iii-worker before the engine test phase and always exercises it).
fn iii_worker_bin() -> Option<PathBuf> {
    let candidate = iii_bin().parent()?.join("iii-worker");
    candidate.exists().then_some(candidate)
}

/// Reserve an ephemeral port. The tiny bind-race window after dropping the
/// listener is acceptable in practice (same pattern as the SDK fixtures).
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind ephemeral port")
        .local_addr()
        .expect("local addr")
        .port()
}

fn read_or_empty(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// Last lines of the engine log, for failure diagnostics.
fn log_tail(path: &Path) -> String {
    let content = read_or_empty(path);
    let lines: Vec<&str> = content.lines().collect();
    let start = lines.len().saturating_sub(30);
    lines[start..].join("\n")
}

/// Base env for every spawned process: sandboxed HOME (so `~/.iii` state,
/// pidfiles, and binary resolution stay inside the tempdir), the sandbox bin
/// dir FIRST on PATH (so the engine's `which("iii-worker")` finds the
/// freshly built binary, not a stale developer install), fail-fast registry
/// URL (builtin adds fire a telemetry POST; 127.0.0.1:1 refuses instantly
/// instead of depending on network), and no inherited config-path override.
fn base_cmd(bin: &Path, home: &Path, cwd: &Path) -> Command {
    let sandbox_bin = home.join(".local/bin");
    let path_var = format!(
        "{}:{}",
        sandbox_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut cmd = Command::new(bin);
    cmd.current_dir(cwd)
        .env("HOME", home)
        .env("PATH", path_var)
        .env("III_TELEMETRY_ENABLED", "false")
        .env("III_API_URL", "http://127.0.0.1:1")
        .env_remove("III_CONFIG_PATH")
        .stdin(Stdio::null());
    cmd
}

#[test]
fn add_with_host_reaches_engine_and_add_without_host_stays_local() {
    let Some(worker_bin) = iii_worker_bin() else {
        eprintln!(
            "skipping remote_worker_add_e2e: iii-worker not built next to {} \
             (run `cargo build -p iii-worker` first)",
            iii_bin().display()
        );
        return;
    };

    // ── Sandbox layout ──────────────────────────────────────────────────
    let sandbox = tempfile::tempdir().expect("tempdir");
    let home = sandbox.path().join("home");
    let engine_dir = sandbox.path().join("engine-dir");
    let other_dir = sandbox.path().join("other-dir");
    let sandbox_bin = home.join(".local/bin");
    for d in [&home, &engine_dir, &other_dir, &sandbox_bin] {
        std::fs::create_dir_all(d).expect("mkdir sandbox");
    }
    // The engine resolves `iii-worker` for its worker-ops daemon via PATH /
    // ~/.local/bin; point both at the freshly built binary.
    std::os::unix::fs::symlink(&worker_bin, sandbox_bin.join("iii-worker"))
        .expect("symlink iii-worker into sandbox bin");

    let port = free_port();
    std::fs::write(
        engine_dir.join("config.yaml"),
        format!(
            "workers:\n  - name: iii-worker-manager\n    config:\n      port: {port}\n      host: 127.0.0.1\n"
        ),
    )
    .expect("write engine config");

    // ── Boot the engine in engine-dir ───────────────────────────────────
    let engine_log = engine_dir.join("engine.log");
    let log_file = std::fs::File::create(&engine_log).expect("create engine log");
    let mut engine_cmd = base_cmd(iii_bin(), &home, &engine_dir);
    engine_cmd
        .arg("--no-update-check")
        .stdout(log_file.try_clone().expect("clone log handle"))
        .stderr(log_file);
    let _engine = KillOnDrop(engine_cmd.spawn().expect("spawn engine"));

    // ── Leg 1: `add --host` from an unrelated directory ─────────────────
    // Retry until the worker-ops daemon has registered worker::add on the
    // engine (it spawns ~2s after engine boot, then connects): early
    // attempts fail with connection refused / function-not-found, both of
    // which simply mean "not ready yet".
    let deadline = Instant::now() + Duration::from_secs(120);
    let last_output = loop {
        let out = base_cmd(&worker_bin, &home, &other_dir)
            .args([
                "add",
                "iii-state",
                "--no-wait",
                "--host",
                &format!("localhost:{port}"),
            ])
            .output()
            .expect("run iii-worker add --host");
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        if out.status.success() {
            break combined;
        }
        assert!(
            Instant::now() < deadline,
            "add --host never succeeded.\nlast output:\n{combined}\nengine log tail:\n{}",
            log_tail(&engine_log)
        );
        std::thread::sleep(Duration::from_secs(1));
    };

    let engine_config = read_or_empty(&engine_dir.join("config.yaml"));
    assert!(
        engine_config.contains("- name: iii-state"),
        "`add --host` must land in the ENGINE's config.yaml.\nengine config:\n{engine_config}\nadd output:\n{last_output}"
    );
    assert!(
        !other_dir.join("config.yaml").exists(),
        "`add --host` must not create a config.yaml in the CLI's cwd"
    );
    assert!(
        !other_dir.join("iii.lock").exists(),
        "`add --host` must not write a lockfile in the CLI's cwd"
    );

    // ── Leg 2: plain `add` (no --host) from that same directory ─────────
    // This is the contract under test: WITHOUT --host there is NO automatic
    // connection to any engine (default host included). The add is a local
    // file edit in the cwd.
    let out = base_cmd(&worker_bin, &home, &other_dir)
        .args(["add", "iii-queue", "--no-wait"])
        .output()
        .expect("run iii-worker add without --host");
    let no_host_output = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.status.success(),
        "plain add of a builtin should succeed offline, got:\n{no_host_output}"
    );

    let local_config = read_or_empty(&other_dir.join("config.yaml"));
    assert!(
        local_config.contains("- name: iii-queue"),
        "plain add must write the CLI cwd's config.yaml, got:\n{local_config}"
    );
    let engine_config = read_or_empty(&engine_dir.join("config.yaml"));
    assert!(
        !engine_config.contains("- name: iii-queue"),
        "plain add must NOT auto-connect to a running engine — the engine's \
         config gained a worker that was added without --host:\n{engine_config}"
    );

    // Belt-and-suspenders for the "no auto-connect" claim: nothing arrives
    // later either (the add already returned; there is no async channel that
    // could deliver it, but a regression that introduced one would flake
    // green without this recheck).
    std::thread::sleep(Duration::from_secs(2));
    let engine_config = read_or_empty(&engine_dir.join("config.yaml"));
    assert!(
        !engine_config.contains("- name: iii-queue"),
        "plain add must never reach the engine, even asynchronously:\n{engine_config}"
    );

    // Flush guard: make failure diagnostics deterministic under --nocapture.
    let _ = std::io::stderr().flush();
}
