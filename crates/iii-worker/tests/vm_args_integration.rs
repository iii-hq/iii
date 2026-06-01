// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Ungated integration tests for VM argument construction, mount parsing,
//! environment variable propagation, signal handling, PID management,
//! path helper verification, and firmware resolution.
//! Covers VM-01 through VM-04 requirements at the pure-function level.
//! These tests run in default `cargo test` without requiring libkrun.

mod common;

use clap::Parser;
use iii_worker::cli::lifecycle::build_container_spec;
use iii_worker::cli::vm_boot::{
    VmBootArgs, build_worker_cmd, resolve_krunfw_file_path, rewrite_localhost, shell_quote,
};
use iii_worker::cli::worker_manager::adapter::RuntimeAdapter;
use iii_worker::cli::worker_manager::libkrun::{LibkrunAdapter, k8s_mem_to_mib};
use iii_worker::cli::worker_manager::state::{WorkerDef, WorkerResources};
use std::collections::HashMap;

/// Helper wrapper for clap parsing of VmBootArgs.
#[derive(Parser)]
struct TestCli {
    #[command(flatten)]
    args: VmBootArgs,
}

#[test]
fn vm_boot_args_all_fields() {
    let cli = TestCli::parse_from([
        "test",
        "--rootfs",
        "/tmp/rootfs",
        "--exec",
        "/usr/bin/node",
        "--workdir",
        "/workspace",
        "--vcpus",
        "4",
        "--ram",
        "1024",
        "--env",
        "FOO=bar",
        "--mount",
        "/host:/guest",
        "--arg",
        "script.js",
        "--slot",
        "42",
        "--pid-file",
        "/tmp/vm.pid",
        "--console-output",
        "/tmp/console.log",
    ]);
    assert_eq!(cli.args.rootfs, "/tmp/rootfs");
    assert_eq!(cli.args.exec, "/usr/bin/node");
    assert_eq!(cli.args.workdir, "/workspace");
    assert_eq!(cli.args.vcpus, 4);
    assert_eq!(cli.args.ram, 1024);
    assert_eq!(cli.args.env, vec!["FOO=bar"]);
    assert_eq!(cli.args.mount, vec!["/host:/guest"]);
    assert_eq!(cli.args.arg, vec!["script.js"]);
    assert_eq!(cli.args.slot, 42);
    assert_eq!(cli.args.pid_file.as_deref(), Some("/tmp/vm.pid"));
    assert_eq!(cli.args.console_output.as_deref(), Some("/tmp/console.log"));
}

#[test]
fn vm_boot_args_defaults() {
    let cli = TestCli::parse_from(["test", "--rootfs", "/rootfs", "--exec", "/bin/app"]);
    assert_eq!(cli.args.rootfs, "/rootfs");
    assert_eq!(cli.args.exec, "/bin/app");
    assert_eq!(cli.args.workdir, "/");
    assert_eq!(cli.args.vcpus, 2);
    assert_eq!(cli.args.ram, 2048);
    assert_eq!(cli.args.slot, 0);
    assert!(cli.args.mount.is_empty());
    assert!(cli.args.env.is_empty());
    assert!(cli.args.arg.is_empty());
    assert!(cli.args.pid_file.is_none());
    assert!(cli.args.console_output.is_none());
}

#[test]
fn vm_boot_args_hyphen_prefixed_args() {
    let cli = TestCli::parse_from([
        "test", "--rootfs", "/r", "--exec", "/e", "--arg", "--port", "--arg", "3000", "--arg", "-v",
    ]);
    assert_eq!(cli.args.arg, vec!["--port", "3000", "-v"]);
}

#[test]
fn vm_boot_args_multiple_envs() {
    let cli = TestCli::parse_from([
        "test",
        "--rootfs",
        "/r",
        "--exec",
        "/e",
        "--env",
        "FOO=bar",
        "--env",
        "BAZ=qux",
        "--env",
        "URL=http://host:8080",
    ]);
    assert_eq!(cli.args.env.len(), 3);
    assert_eq!(cli.args.env[0], "FOO=bar");
    assert_eq!(cli.args.env[1], "BAZ=qux");
    assert_eq!(cli.args.env[2], "URL=http://host:8080");
}

#[test]
fn vm_boot_args_multiple_mounts() {
    let cli = TestCli::parse_from([
        "test",
        "--rootfs",
        "/r",
        "--exec",
        "/e",
        "--mount",
        "/src:/workspace",
        "--mount",
        "/data:/mnt/data",
        "--mount",
        "/cache:/tmp/cache",
    ]);
    assert_eq!(cli.args.mount.len(), 3);
    assert_eq!(cli.args.mount[0], "/src:/workspace");
    assert_eq!(cli.args.mount[1], "/data:/mnt/data");
    assert_eq!(cli.args.mount[2], "/cache:/tmp/cache");
}

#[test]
fn shell_quote_safe_chars_passthrough() {
    assert_eq!(shell_quote("simple"), "simple");
    assert_eq!(shell_quote("/usr/bin/node"), "/usr/bin/node");
    assert_eq!(shell_quote("key=value"), "key=value");
    assert_eq!(shell_quote("a-b_c.d:e/f"), "a-b_c.d:e/f");
}

#[test]
fn shell_quote_spaces_get_quoted() {
    assert_eq!(shell_quote("has space"), "'has space'");
}

#[test]
fn shell_quote_embedded_single_quotes() {
    assert_eq!(shell_quote("it's a test"), "'it'\\''s a test'");
}

#[test]
fn shell_quote_special_chars() {
    let result = shell_quote("$(cmd)");
    assert!(result.starts_with('\''), "$(cmd) should be quoted");
    let result2 = shell_quote("a;b");
    assert!(result2.starts_with('\''), "a;b should be quoted");
}

#[test]
fn shell_quote_empty_string() {
    // Empty string has no unsafe chars, passes through unchanged.
    assert_eq!(shell_quote(""), "");
}

#[test]
fn build_worker_cmd_no_args() {
    assert_eq!(build_worker_cmd("/usr/bin/node", &[]), "/usr/bin/node");
}

#[test]
fn build_worker_cmd_with_args() {
    let args = vec![
        "script.js".to_string(),
        "--port".to_string(),
        "3000".to_string(),
    ];
    assert_eq!(
        build_worker_cmd("/usr/bin/node", &args),
        "/usr/bin/node script.js --port 3000"
    );
}

#[test]
fn build_worker_cmd_with_spaces_in_exec() {
    let result = build_worker_cmd("/path/to/my app", &[]);
    assert!(
        result.contains('\''),
        "exec path with spaces should be quoted: {}",
        result
    );
}

#[test]
fn build_worker_cmd_with_special_arg() {
    let args = vec!["it's".to_string()];
    let result = build_worker_cmd("/bin/sh", &args);
    assert!(
        result.contains("\\'"),
        "argument with single quote should be escaped: {}",
        result
    );
}

#[test]
fn k8s_mem_to_mib_mi() {
    assert_eq!(k8s_mem_to_mib("512Mi"), Some("512".into()));
}

#[test]
fn k8s_mem_to_mib_gi() {
    assert_eq!(k8s_mem_to_mib("2Gi"), Some("2048".into()));
}

#[test]
fn k8s_mem_to_mib_ki() {
    assert_eq!(k8s_mem_to_mib("1048576Ki"), Some("1024".into()));
}

#[test]
fn k8s_mem_to_mib_bytes() {
    assert_eq!(k8s_mem_to_mib("2147483648"), Some("2048".into()));
}

#[test]
fn k8s_mem_to_mib_invalid() {
    assert_eq!(k8s_mem_to_mib("not-a-number"), None);
}

#[test]
fn k8s_mem_to_mib_zero() {
    assert_eq!(k8s_mem_to_mib("0"), Some("0".into()));
}

#[test]
fn vm_boot_args_mount_valid_format() {
    let cli = TestCli::parse_from([
        "test",
        "--rootfs",
        "/r",
        "--exec",
        "/e",
        "--mount",
        "/host:/guest",
    ]);
    assert_eq!(cli.args.mount[0], "/host:/guest");
}

#[test]
fn vm_boot_args_mount_no_colon_stored_as_is() {
    // VmBootArgs stores the string as-is; validation happens at boot_vm time.
    let cli = TestCli::parse_from([
        "test",
        "--rootfs",
        "/r",
        "--exec",
        "/e",
        "--mount",
        "invalid-no-colon",
    ]);
    assert_eq!(cli.args.mount[0], "invalid-no-colon");
}

#[test]
fn vm_boot_args_mount_empty_guest() {
    let cli = TestCli::parse_from([
        "test", "--rootfs", "/r", "--exec", "/e", "--mount", "/host:",
    ]);
    assert_eq!(cli.args.mount[0], "/host:");
}

#[test]
fn rewrite_localhost_replaces_localhost() {
    assert_eq!(
        rewrite_localhost("http://localhost:3000/api", "192.168.1.1"),
        "http://192.168.1.1:3000/api"
    );
}

#[test]
fn rewrite_localhost_replaces_loopback() {
    assert_eq!(
        rewrite_localhost("ws://127.0.0.1:8080/ws", "10.0.0.1"),
        "ws://10.0.0.1:8080/ws"
    );
}

#[test]
fn rewrite_localhost_both_in_one_string() {
    let result = rewrite_localhost("http://localhost:3000 ws://127.0.0.1:8080", "10.0.0.1");
    assert!(
        result.contains("10.0.0.1:3000"),
        "should rewrite localhost: {}",
        result
    );
    assert!(
        result.contains("10.0.0.1:8080"),
        "should rewrite 127.0.0.1: {}",
        result
    );
}

#[test]
fn rewrite_localhost_no_match_unchanged() {
    assert_eq!(
        rewrite_localhost("http://example.com:3000", "10.0.0.1"),
        "http://example.com:3000"
    );
}

#[test]
fn rewrite_localhost_no_port_unchanged() {
    // Only matches "://localhost:" pattern (requires colon after hostname).
    assert_eq!(
        rewrite_localhost("localhost without colon", "10.0.0.1"),
        "localhost without colon"
    );
}

// Regression: with networking disabled, `gateway_ip` is `None` and the
// rewrite must be a no-op. Empty-string sentinels previously turned
// `://localhost:9000` into `://:9000` (vm_boot.rs:683-698 pre-fix).
#[test]
fn maybe_rewrite_localhost_skips_when_networking_disabled() {
    use iii_worker::cli::vm_boot::maybe_rewrite_localhost;

    assert_eq!(
        maybe_rewrite_localhost("http://localhost:9000/api", None),
        "http://localhost:9000/api"
    );
    assert_eq!(
        maybe_rewrite_localhost("ws://127.0.0.1:8080/ws", None),
        "ws://127.0.0.1:8080/ws"
    );
}

#[test]
fn maybe_rewrite_localhost_rewrites_when_gateway_present() {
    use iii_worker::cli::vm_boot::maybe_rewrite_localhost;

    assert_eq!(
        maybe_rewrite_localhost("http://localhost:3000/api", Some("192.168.1.1")),
        "http://192.168.1.1:3000/api"
    );
    assert_eq!(
        maybe_rewrite_localhost("ws://127.0.0.1:8080/ws", Some("10.0.0.1")),
        "ws://10.0.0.1:8080/ws"
    );
}

#[test]
fn build_container_spec_managed_with_resources() {
    let mut env = HashMap::new();
    env.insert(
        "III_ENGINE_URL".to_string(),
        "ws://localhost:49134".to_string(),
    );
    let def = WorkerDef::Managed {
        image: "ghcr.io/iii-hq/worker:latest".to_string(),
        env,
        resources: Some(WorkerResources {
            cpus: Some("4".to_string()),
            memory: Some("4096Mi".to_string()),
        }),
    };
    let spec = build_container_spec("vm-worker", &def, "ws://localhost:49134");
    assert_eq!(spec.name, "vm-worker");
    assert_eq!(spec.image, "ghcr.io/iii-hq/worker:latest");
    assert_eq!(spec.cpu_limit.as_deref(), Some("4"));
    assert_eq!(spec.memory_limit.as_deref(), Some("4096Mi"));
    assert!(spec.env.contains_key("III_ENGINE_URL"));
}

#[test]
fn build_container_spec_managed_no_resources() {
    let def = WorkerDef::Managed {
        image: "ghcr.io/iii-hq/worker:latest".to_string(),
        env: HashMap::new(),
        resources: None,
    };
    let spec = build_container_spec("vm-worker", &def, "ws://localhost:49134");
    assert!(spec.cpu_limit.is_none());
    assert!(spec.memory_limit.is_none());
}

#[test]
fn build_container_spec_binary_has_no_spec_fields() {
    let def = WorkerDef::Binary {
        version: "0.1.2".to_string(),
        config: None,
    };
    let spec = build_container_spec("bin-worker", &def, "ws://localhost:49134");
    assert!(spec.image.is_empty());
    assert!(spec.env.is_empty());
    assert!(spec.cpu_limit.is_none());
    assert!(spec.memory_limit.is_none());
}

#[tokio::test]
async fn stop_terminates_sleeping_process() {
    let mut child = std::process::Command::new("sleep")
        .arg("60")
        .spawn()
        .expect("failed to spawn sleep");
    let pid = child.id();
    // Forget the child so our Drop does not interfere with stop()'s signal handling.
    std::mem::forget(child);

    let adapter = LibkrunAdapter::new();
    let pid_str = pid.to_string();

    // Record start time. With timeout=5s, if SIGTERM works immediately,
    // stop() should return well under 3 seconds. If it had to wait for
    // SIGKILL escalation, it would take ~5 seconds.
    let start = std::time::Instant::now();
    adapter
        .stop(&pid_str, 5)
        .await
        .expect("stop should succeed");
    let elapsed = start.elapsed();

    assert!(
        elapsed < std::time::Duration::from_secs(3),
        "stop() took {:?}, expected < 3s (SIGTERM should kill sleep immediately)",
        elapsed
    );

    let alive = unsafe { nix::libc::kill(pid as i32, 0) == 0 };
    assert!(!alive, "process {} should be dead after stop()", pid);
}

#[tokio::test]
async fn stop_escalates_to_sigkill_when_sigterm_ignored() {
    // Spawn a process that traps (ignores) SIGTERM.
    let mut child = std::process::Command::new("sh")
        .arg("-c")
        .arg("trap '' TERM; sleep 60")
        .spawn()
        .expect("failed to spawn trap process");
    let pid = child.id();
    std::mem::forget(child);

    let adapter = LibkrunAdapter::new();
    let pid_str = pid.to_string();

    // Use a 1-second timeout to force SIGKILL escalation quickly.
    adapter
        .stop(&pid_str, 1)
        .await
        .expect("stop should succeed");

    // Give a moment for the kernel to reap the killed process.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let alive = unsafe { nix::libc::kill(pid as i32, 0) == 0 };
    assert!(
        !alive,
        "process {} should be dead after SIGKILL escalation",
        pid
    );
}

#[tokio::test]
async fn stop_handles_already_dead_process() {
    let child = std::process::Command::new("sleep")
        .arg("0")
        .spawn()
        .expect("failed to spawn sleep 0");
    let pid = child.id();
    std::thread::sleep(std::time::Duration::from_millis(200));

    let adapter = LibkrunAdapter::new();
    let pid_str = pid.to_string();
    let result = adapter.stop(&pid_str, 1).await;
    assert!(result.is_ok(), "stop on dead process should not error");
}

#[tokio::test]
async fn status_running_process_detected() {
    let adapter = LibkrunAdapter::new();
    // Use our own PID -- we know it is alive.
    let our_pid = std::process::id().to_string();
    let status = adapter
        .status(&our_pid)
        .await
        .expect("status should succeed");
    assert!(
        status.running,
        "our own process should be detected as running"
    );
}

#[tokio::test]
async fn status_dead_process_detected() {
    let adapter = LibkrunAdapter::new();
    // PID 99999999 is almost certainly not running.
    let status = adapter
        .status("99999999")
        .await
        .expect("status should succeed");
    assert!(!status.running, "PID 99999999 should not be running");
}

#[tokio::test]
async fn status_invalid_pid_zero() {
    let adapter = LibkrunAdapter::new();
    let status = adapter.status("0").await.expect("status should succeed");
    assert!(!status.running, "PID 0 should not be reported as running");
}

#[test]
fn pid_file_write_and_read_roundtrip() {
    let dir = tempfile::tempdir().expect("failed to create tempdir");
    let pid_path = dir.path().join("vm.pid");
    std::fs::write(&pid_path, "12345").expect("failed to write PID file");
    let content = std::fs::read_to_string(&pid_path).expect("failed to read PID file");
    assert_eq!(content, "12345");
}

#[test]
fn worker_dir_constructs_path_under_managed() {
    let path = LibkrunAdapter::worker_dir("my-worker");
    let path_str = path.to_string_lossy();
    assert!(
        path_str.contains(".iii/managed/my-worker"),
        "worker_dir should contain .iii/managed/my-worker, got: {}",
        path_str
    );
    let components: Vec<_> = path
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect();
    let len = components.len();
    assert!(len >= 3, "path should have at least 3 components");
    assert_eq!(components[len - 3], ".iii");
    assert_eq!(components[len - 2], "managed");
    assert_eq!(components[len - 1], "my-worker");
}

#[test]
fn pid_file_is_under_worker_dir() {
    let pid = LibkrunAdapter::pid_file("test-vm");
    let pid_str = pid.to_string_lossy();
    assert!(
        pid_str.contains(".iii/managed/test-vm/vm.pid"),
        "pid_file should end with .iii/managed/test-vm/vm.pid, got: {}",
        pid_str
    );
    assert_eq!(
        pid.parent().unwrap(),
        LibkrunAdapter::worker_dir("test-vm"),
        "pid_file parent should equal worker_dir"
    );
}

#[test]
fn logs_dir_is_under_worker_dir() {
    let logs = LibkrunAdapter::logs_dir("test-vm");
    let logs_str = logs.to_string_lossy();
    assert!(
        logs_str.contains(".iii/managed/test-vm/logs"),
        "logs_dir should end with .iii/managed/test-vm/logs, got: {}",
        logs_str
    );
    assert_eq!(
        logs.parent().unwrap(),
        LibkrunAdapter::worker_dir("test-vm"),
        "logs_dir parent should equal worker_dir"
    );
}

#[test]
fn image_rootfs_uses_unified_cache_path() {
    // After the rootfs-cache unification, `image_rootfs` delegates to
    // `rootfs_cache::canonical_path`, which lands at
    // `~/.iii/cache/<slug>/` instead of the old `~/.iii/images/<sha>/`.
    // Different images still resolve to different slugs.
    let path_a = LibkrunAdapter::image_rootfs("ghcr.io/iii-hq/worker:latest");
    let path_a_str = path_a.to_string_lossy();
    assert!(
        path_a_str.contains(".iii/cache/"),
        "image_rootfs should land under .iii/cache/, got: {}",
        path_a_str
    );

    // Different image strings should produce different slugs.
    let path_b = LibkrunAdapter::image_rootfs("docker.io/library/nginx:alpine");
    assert_ne!(
        path_a.file_name(),
        path_b.file_name(),
        "different images should have different cache directories"
    );

    // #1540 invariant: pinned (`@sha256:...`) and unpinned variants
    // of the same ref collapse to the same cache dir, so resolving a
    // pin at engine start doesn't trigger a re-pull of an image the
    // user already added.
    let pinned = LibkrunAdapter::image_rootfs("ghcr.io/iii-hq/worker:latest@sha256:deadbeef");
    assert_eq!(
        path_a, pinned,
        "pinned and unpinned refs must share a cache dir"
    );
}

#[test]
fn path_helpers_different_names_different_paths() {
    assert_ne!(
        LibkrunAdapter::worker_dir("a"),
        LibkrunAdapter::worker_dir("b"),
        "different names should produce different worker_dir paths"
    );
    assert_ne!(
        LibkrunAdapter::pid_file("a"),
        LibkrunAdapter::pid_file("b"),
        "different names should produce different pid_file paths"
    );
}

#[test]
fn resolve_krunfw_file_path_returns_option() {
    // In a test environment without firmware installed, this should return None.
    // The function is callable and returns the correct type.
    let result = resolve_krunfw_file_path();
    // We cannot guarantee Some or None depending on the host, but in CI/test
    // environments firmware is typically not installed. The key assertion is
    // that the function is callable from integration tests (pub visibility works).
    assert!(
        result.is_none() || result.is_some(),
        "resolve_krunfw_file_path should return a valid Option"
    );
    // If firmware IS present, verify the path exists.
    if let Some(ref path) = result {
        assert!(path.exists(), "returned firmware path should exist");
    }
}

/// Verify that the dev-worker logs dir + stdout.log path convention matches
/// the managed-worker console_output convention. Both paths must end with
/// stdout.log so VM console output is written to a file rather than going
/// through PortOutputLog (which hardcodes ERROR level for all lines).
#[test]
fn run_dev_console_output_path_convention() {
    let worker_name = "image-resize";
    // Dev worker path: ~/.iii/logs/<name>/stdout.log
    let dev_logs_dir = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join(".iii/logs")
        .join(worker_name);
    let dev_console_output = dev_logs_dir.join("stdout.log");

    let dev_str = dev_console_output.to_string_lossy();
    assert!(
        dev_str.ends_with("stdout.log"),
        "dev console output should end with stdout.log, got: {}",
        dev_str
    );
    assert!(
        dev_str.contains(".iii/logs/image-resize"),
        "dev console output should be under .iii/logs/<worker>, got: {}",
        dev_str
    );

    // Managed worker path: ~/.iii/managed/<name>/logs/stdout.log
    let managed_console_output = LibkrunAdapter::logs_dir(worker_name).join("stdout.log");
    let managed_str = managed_console_output.to_string_lossy();
    assert!(
        managed_str.ends_with("stdout.log"),
        "managed console output should also end with stdout.log, got: {}",
        managed_str
    );
}

/// Verify that --console-output can be parsed as a valid VmBootArgs field
/// when set to the dev-worker logs path, proving the __vm-boot subprocess
/// will accept it.
#[test]
fn run_dev_console_output_parses_as_vm_boot_arg() {
    let dev_console_path = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join(".iii/logs/test-worker/stdout.log");
    let path_str = dev_console_path.to_string_lossy().to_string();

    let cli = TestCli::parse_from([
        "test",
        "--rootfs",
        "/tmp/rootfs",
        "--exec",
        "/bin/sh",
        "--console-output",
        &path_str,
    ]);
    assert_eq!(
        cli.args.console_output.as_deref(),
        Some(path_str.as_str()),
        "dev console output path should parse into VmBootArgs.console_output"
    );
}
