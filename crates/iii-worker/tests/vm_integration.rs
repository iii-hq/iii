// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Feature-gated VM integration tests.
//! Run with: cargo test -p iii-worker --test vm_integration --features integration-vm
//!
//! These tests verify the argument construction patterns used by LibkrunAdapter::start(),
//! environment variable merging, path conventions, adapter lifecycle contracts, and
//! actual __vm-boot subprocess argument passing.

#![cfg(feature = "integration-vm")]

mod common;

use clap::Parser;
use iii_worker::cli::vm_boot::{VmBootArgs, rewrite_localhost};
use iii_worker::cli::worker_manager::adapter::RuntimeAdapter;
use iii_worker::cli::worker_manager::libkrun::LibkrunAdapter;
use std::collections::HashMap;

/// Helper wrapper for clap parsing of VmBootArgs.
#[derive(Parser)]
struct TestCli {
    #[command(flatten)]
    args: VmBootArgs,
}

// ===========================================================================
// Group 1: Feature gate verification (sanity check)
// ===========================================================================

#[test]
fn vm_feature_gate_compiles() {
    assert!(
        cfg!(feature = "integration-vm"),
        "integration-vm feature should be active when this file compiles"
    );
}

// ===========================================================================
// Group 2: Realistic VmBootArgs matching start() pattern (VM-01, per D-07)
// ===========================================================================

/// Parse args mimicking what LibkrunAdapter::start() actually constructs:
/// rootfs, exec, workdir, vcpus, ram, env vars, mount, args, pid-file,
/// console-output, and slot -- matching the real argument list from start().
#[test]
fn vm_boot_args_realistic_managed_worker() {
    let cli = TestCli::parse_from([
        "test",
        "--rootfs",
        "/home/user/.iii/managed/my-worker/rootfs",
        "--exec",
        "/usr/bin/node",
        "--workdir",
        "/workspace",
        "--vcpus",
        "4",
        "--ram",
        "4096",
        "--env",
        "III_ENGINE_URL=ws://localhost:49134",
        "--env",
        "III_WORKER_NAME=my-worker",
        "--mount",
        "/home/user/project:/workspace",
        "--arg",
        "--url",
        "--arg",
        "ws://localhost:49134",
        "--pid-file",
        "/home/user/.iii/managed/my-worker/vm.pid",
        "--console-output",
        "/home/user/.iii/managed/my-worker/logs/stdout.log",
        "--slot",
        "1",
    ]);
    assert_eq!(cli.args.rootfs, "/home/user/.iii/managed/my-worker/rootfs");
    assert_eq!(cli.args.exec, "/usr/bin/node");
    assert_eq!(cli.args.workdir, "/workspace");
    assert_eq!(cli.args.vcpus, 4);
    assert_eq!(cli.args.ram, 4096);
    assert_eq!(
        cli.args.env,
        vec![
            "III_ENGINE_URL=ws://localhost:49134",
            "III_WORKER_NAME=my-worker"
        ]
    );
    assert_eq!(cli.args.mount, vec!["/home/user/project:/workspace"]);
    assert_eq!(cli.args.arg, vec!["--url", "ws://localhost:49134"]);
    assert_eq!(
        cli.args.pid_file.as_deref(),
        Some("/home/user/.iii/managed/my-worker/vm.pid")
    );
    assert_eq!(
        cli.args.console_output.as_deref(),
        Some("/home/user/.iii/managed/my-worker/logs/stdout.log")
    );
    assert_eq!(cli.args.slot, 1);
}

/// Verify that the PID file path convention from start() matches the
/// LibkrunAdapter::pid_file() helper.
#[test]
fn vm_boot_args_start_uses_correct_pid_file_path() {
    let expected_pid_path = LibkrunAdapter::pid_file("test-worker");
    let expected_pid_str = expected_pid_path.to_string_lossy().to_string();

    let cli = TestCli::parse_from([
        "test",
        "--rootfs",
        "/tmp/rootfs",
        "--exec",
        "/bin/sh",
        "--pid-file",
        &expected_pid_str,
    ]);
    assert_eq!(
        cli.args.pid_file.as_deref(),
        Some(expected_pid_str.as_str()),
        "parsed --pid-file should match LibkrunAdapter::pid_file()"
    );
    // Verify the path follows the worker_dir/vm.pid convention
    assert!(
        expected_pid_str.ends_with("vm.pid"),
        "pid_file should end with vm.pid"
    );
    assert!(
        expected_pid_str.contains(".iii/managed/test-worker"),
        "pid_file should be under .iii/managed/test-worker"
    );
}

/// Verify that the console output path convention from start() matches
/// the logs_dir pattern: logs_dir(name).join("stdout.log").
#[test]
fn vm_boot_args_start_uses_correct_console_output_path() {
    let expected_log_path = LibkrunAdapter::logs_dir("test-worker").join("stdout.log");
    let expected_log_str = expected_log_path.to_string_lossy().to_string();

    let cli = TestCli::parse_from([
        "test",
        "--rootfs",
        "/tmp/rootfs",
        "--exec",
        "/bin/sh",
        "--console-output",
        &expected_log_str,
    ]);
    assert_eq!(
        cli.args.console_output.as_deref(),
        Some(expected_log_str.as_str()),
        "parsed --console-output should match logs_dir/stdout.log"
    );
    // Verify the path follows the expected convention
    assert!(
        expected_log_str.ends_with("logs/stdout.log"),
        "console_output should end with logs/stdout.log"
    );
    assert!(
        expected_log_str.contains(".iii/managed/test-worker"),
        "console_output should be under .iii/managed/test-worker"
    );
}

/// Parse with vcpus at u8 boundary and beyond. VmBootArgs accepts u32;
/// the u8 check happens at boot_vm runtime, not at parse time.
#[test]
fn vm_boot_args_max_vcpus_at_u8_boundary() {
    // u8::MAX (255) should parse fine
    let cli = TestCli::parse_from(["test", "--rootfs", "/r", "--exec", "/e", "--vcpus", "255"]);
    assert_eq!(cli.args.vcpus, 255);

    // 256 should also parse (VmBootArgs field is u32)
    let cli = TestCli::parse_from(["test", "--rootfs", "/r", "--exec", "/e", "--vcpus", "256"]);
    assert_eq!(cli.args.vcpus, 256);

    // Large value within u32 range
    let cli = TestCli::parse_from(["test", "--rootfs", "/r", "--exec", "/e", "--vcpus", "65535"]);
    assert_eq!(cli.args.vcpus, 65535);
}

// ===========================================================================
// Group 3: Environment variable merging pattern (VM-03, per D-09)
// ===========================================================================

/// Simulate the env merging pattern from LibkrunAdapter::start() lines 422-429.
/// Image env is loaded first, then spec env overwrites matching keys.
#[test]
fn env_merger_image_env_overridden_by_spec_env() {
    // Simulate image env (from OCI config)
    let mut image_env: HashMap<String, String> = HashMap::new();
    image_env.insert("PATH".to_string(), "/usr/bin".to_string());
    image_env.insert("LANG".to_string(), "C".to_string());

    // Simulate spec env (from ContainerSpec)
    let mut spec_env: HashMap<String, String> = HashMap::new();
    spec_env.insert("PATH".to_string(), "/custom/bin".to_string());
    spec_env.insert("III_URL".to_string(), "ws://host:1234".to_string());

    // Merge: start with image_env, then spec overwrites (exact pattern from start())
    let mut merged_env: HashMap<String, String> = image_env.into_iter().collect();
    for (key, value) in &spec_env {
        merged_env.insert(key.clone(), value.clone());
    }

    // spec's PATH should overwrite image's PATH
    assert_eq!(
        merged_env.get("PATH").unwrap(),
        "/custom/bin",
        "spec env should overwrite image env for PATH"
    );
    // III_URL from spec should be present
    assert_eq!(
        merged_env.get("III_URL").unwrap(),
        "ws://host:1234",
        "spec env should add III_URL"
    );
    // LANG from image should be preserved (not in spec)
    assert_eq!(
        merged_env.get("LANG").unwrap(),
        "C",
        "image env LANG should be preserved when not overridden"
    );
    // Total: 3 keys (PATH overwritten, LANG preserved, III_URL added)
    assert_eq!(merged_env.len(), 3);
}

/// Verify that rewrite_localhost correctly transforms environment variable
/// values containing localhost URLs, as happens during VM boot.
#[test]
fn env_values_with_localhost_get_rewritten() {
    let env_vars: Vec<(&str, &str)> = vec![
        ("III_ENGINE_URL", "ws://localhost:49134"),
        ("DATABASE_URL", "postgres://127.0.0.1:5432/db"),
        ("SIMPLE_VAR", "no-url-here"),
    ];
    let gateway_ip = "192.168.64.2";

    let rewritten: Vec<(String, String)> = env_vars
        .iter()
        .map(|(k, v)| (k.to_string(), rewrite_localhost(v, gateway_ip)))
        .collect();

    // III_ENGINE_URL: localhost -> gateway_ip
    let iii_url = &rewritten[0].1;
    assert!(
        iii_url.contains("192.168.64.2:49134"),
        "III_ENGINE_URL should have rewritten localhost: {}",
        iii_url
    );
    assert!(
        !iii_url.contains("localhost"),
        "III_ENGINE_URL should not contain localhost after rewrite: {}",
        iii_url
    );

    // DATABASE_URL: 127.0.0.1 -> gateway_ip
    let db_url = &rewritten[1].1;
    assert!(
        db_url.contains("192.168.64.2:5432"),
        "DATABASE_URL should have rewritten 127.0.0.1: {}",
        db_url
    );
    assert!(
        !db_url.contains("127.0.0.1"),
        "DATABASE_URL should not contain 127.0.0.1 after rewrite: {}",
        db_url
    );

    // SIMPLE_VAR: no URL, unchanged
    assert_eq!(
        rewritten[2].1, "no-url-here",
        "SIMPLE_VAR should be unchanged"
    );
}

// ===========================================================================
// Group 4: LibkrunAdapter lifecycle contracts (VM-04)
// ===========================================================================

/// LibkrunAdapter is a zero-sized unit struct -- construction requires no
/// runtime dependencies. Its presence in a gated file proves the import
/// path compiles when integration-vm is active.
#[test]
fn libkrun_adapter_constructor_is_zero_cost() {
    let adapter = LibkrunAdapter::new();
    // Also verify Default impl works
    let _adapter2: LibkrunAdapter = Default::default();
    // The adapter is a unit struct -- no fields to inspect.
    // The assertions are that construction succeeds without panic.
    let _ = adapter;
}

/// stop() should be a no-op for non-existent PIDs. status() should report
/// not-running for a PID that does not exist.
#[tokio::test]
async fn stop_and_status_contract_on_bogus_pid() {
    let adapter = LibkrunAdapter::new();

    // stop on a PID that almost certainly does not exist
    let result = adapter.stop("99999999", 1).await;
    assert!(
        result.is_ok(),
        "stop() on non-existent PID should return Ok: {:?}",
        result
    );

    // status on a PID that almost certainly does not exist
    let status = adapter
        .status("99999999")
        .await
        .expect("status() should succeed");
    assert!(
        !status.running,
        "status() for non-existent PID should report not running"
    );
    assert_eq!(
        status.container_id, "99999999",
        "status should echo back the container_id"
    );
}

/// remove() should succeed even when no matching PID file exists in the
/// managed directory.
#[tokio::test]
async fn remove_with_nonexistent_worker_succeeds() {
    let adapter = LibkrunAdapter::new();
    let result = adapter.remove("99999999").await;
    assert!(
        result.is_ok(),
        "remove() on non-existent worker should return Ok: {:?}",
        result
    );
}

// ===========================================================================
// Group 5: Actual subprocess argument verification (VM-01, per D-05/D-07)
// ===========================================================================

/// Spawn the real iii-worker __vm-boot subprocess with a valid temp directory
/// as rootfs. The subprocess will fail because the rootfs has no init binary
/// and no firmware, but its stderr output will contain the rootfs path --
/// proving it received and used the --rootfs argument correctly.
#[test]
fn vm_boot_subprocess_receives_correct_rootfs_argument() {
    let temp_dir = tempfile::TempDir::new().expect("failed to create temp dir");
    let temp_dir_path = temp_dir.path().to_string_lossy().to_string();

    let binary = env!("CARGO_BIN_EXE_iii-worker");
    let output = std::process::Command::new(binary)
        .arg("__vm-boot")
        .arg("--rootfs")
        .arg(&temp_dir_path)
        .arg("--exec")
        .arg("/usr/bin/node")
        .arg("--workdir")
        .arg("/workspace")
        .arg("--vcpus")
        .arg("2")
        .arg("--ram")
        .arg("2048")
        .arg("--env")
        .arg("III_ENGINE_URL=ws://localhost:49134")
        .arg("--env")
        .arg("III_WORKER_NAME=test-subprocess")
        .arg("--arg")
        .arg("--url")
        .arg("--arg")
        .arg("ws://localhost:49134")
        .output()
        .expect("failed to execute iii-worker __vm-boot subprocess");

    // The subprocess should exit with non-zero status (no init binary)
    assert!(
        !output.status.success(),
        "subprocess should fail (no init binary in rootfs)"
    );

    let stderr_str = String::from_utf8_lossy(&output.stderr);

    // The key assertion: stderr contains the rootfs path, proving the
    // subprocess received the --rootfs argument and attempted to use it.
    // The error comes from vm_boot::run() or boot_vm() which includes
    // the rootfs path in its error message.
    assert!(
        stderr_str.contains(&temp_dir_path),
        "stderr should contain the rootfs path '{}' proving argument receipt.\nActual stderr: {}",
        temp_dir_path,
        stderr_str
    );

    // Verify the error message is from the expected validation path
    let is_known_error = stderr_str.contains("No init binary available")
        || stderr_str.contains("VM execution failed")
        || stderr_str.contains("PassthroughFs failed");
    assert!(
        is_known_error,
        "stderr should contain a known VM boot error message.\nActual stderr: {}",
        stderr_str
    );
}

/// Spawn the real iii-worker __vm-boot subprocess with a nonexistent rootfs
/// path. The subprocess should fail immediately with the early validation
/// error in vm_boot::run() and report the path in stderr.
#[test]
fn vm_boot_subprocess_rejects_nonexistent_rootfs() {
    let binary = env!("CARGO_BIN_EXE_iii-worker");
    let nonexistent_path = "/nonexistent/path/that/does/not/exist";

    let output = std::process::Command::new(binary)
        .arg("__vm-boot")
        .arg("--rootfs")
        .arg(nonexistent_path)
        .arg("--exec")
        .arg("/bin/sh")
        .arg("--vcpus")
        .arg("1")
        .arg("--ram")
        .arg("512")
        .output()
        .expect("failed to execute iii-worker __vm-boot subprocess");

    // The subprocess should exit with non-zero status
    assert!(
        !output.status.success(),
        "subprocess should fail for nonexistent rootfs"
    );

    let stderr_str = String::from_utf8_lossy(&output.stderr);

    // Verify the early validation message from run() line 143-145
    assert!(
        stderr_str.contains("rootfs path does not exist"),
        "stderr should contain 'rootfs path does not exist'.\nActual stderr: {}",
        stderr_str
    );

    // Verify the nonexistent path appears in stderr
    assert!(
        stderr_str.contains(nonexistent_path),
        "stderr should contain the nonexistent path '{}'.\nActual stderr: {}",
        nonexistent_path,
        stderr_str
    );
}
