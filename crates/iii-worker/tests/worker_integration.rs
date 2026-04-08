//! Integration tests for iii-worker.
//!
//! These tests import the real `Cli`, `Commands`, and `VmBootArgs` types from
//! the crate library, ensuring any CLI changes are caught at compile time.

use clap::Parser;
#[cfg(all(target_os = "linux", not(target_env = "musl")))]
use iii_worker::VmBootArgs;
use iii_worker::{Cli, Commands, DEFAULT_PORT};

/// All 10 subcommands parse without error.
#[test]
fn cli_parses_all_subcommands() {
    #[allow(unused_mut)]
    let mut cases: Vec<(&[&str], fn(Commands))> = vec![
        (&["iii-worker", "add", "pdfkit@1.0.0"], |c| {
            assert!(matches!(c, Commands::Add { .. }))
        }),
        (&["iii-worker", "remove", "pdfkit"], |c| {
            assert!(matches!(c, Commands::Remove { .. }))
        }),
        (&["iii-worker", "start", "pdfkit"], |c| {
            assert!(matches!(c, Commands::Start { .. }))
        }),
        (&["iii-worker", "stop", "pdfkit"], |c| {
            assert!(matches!(c, Commands::Stop { .. }))
        }),
        (&["iii-worker", "dev", "."], |c| {
            assert!(matches!(c, Commands::Dev { .. }))
        }),
        (&["iii-worker", "list"], |c| {
            assert!(matches!(c, Commands::List))
        }),
        (&["iii-worker", "logs", "my-worker"], |c| {
            assert!(matches!(c, Commands::Logs { .. }))
        }),
    ];

    #[cfg(all(target_os = "linux", not(target_env = "musl")))]
    cases.push((
        &[
            "iii-worker",
            "__vm-boot",
            "--rootfs",
            "/tmp/rootfs",
            "--exec",
            "/usr/bin/node",
        ],
        |c| assert!(matches!(c, Commands::VmBoot(_))),
    ));

    for (args, check) in cases {
        let cli = Cli::try_parse_from(args)
            .unwrap_or_else(|e| panic!("failed to parse {:?}: {}", args, e));
        check(cli.command);
    }
}

/// `add` subcommand parses worker name and applies defaults.
#[test]
fn add_subcommand_fields() {
    let cli = Cli::parse_from(["iii-worker", "add", "ghcr.io/iii-hq/node:latest"]);
    match cli.command {
        Commands::Add {
            worker_name,
            runtime,
            address,
            port,
        } => {
            assert_eq!(worker_name, "ghcr.io/iii-hq/node:latest");
            assert_eq!(runtime, "libkrun");
            assert_eq!(address, "localhost");
            assert_eq!(port, DEFAULT_PORT);
        }
        _ => panic!("expected Add"),
    }
}

/// `dev` subcommand requires a path and supports all optional flags.
#[test]
fn dev_subcommand_all_flags() {
    let cli = Cli::parse_from([
        "iii-worker",
        "dev",
        "/tmp/project",
        "--rebuild",
        "--name",
        "my-worker",
        "--port",
        "5000",
    ]);
    match cli.command {
        Commands::Dev {
            path,
            name,
            rebuild,
            port,
            ..
        } => {
            assert_eq!(path, "/tmp/project");
            assert_eq!(name, Some("my-worker".to_string()));
            assert!(rebuild);
            assert_eq!(port, 5000);
        }
        _ => panic!("expected Dev"),
    }
}

/// `dev` without a path argument fails (path is required).
#[test]
fn dev_requires_path() {
    let result = Cli::try_parse_from(["iii-worker", "dev"]);
    assert!(result.is_err(), "dev without PATH should fail");
}

/// `logs` subcommand parses worker name and --follow flag.
#[test]
fn logs_subcommand_with_follow() {
    let cli = Cli::parse_from(["iii-worker", "logs", "image-resize", "--follow"]);
    match cli.command {
        Commands::Logs {
            worker_name,
            follow,
            ..
        } => {
            assert_eq!(worker_name, "image-resize");
            assert!(follow);
        }
        _ => panic!("expected Logs"),
    }
}

/// `VmBootArgs` roundtrip with all fields including `mount`, `pid_file`,
/// `console_output`, and `slot`.
#[cfg(all(target_os = "linux", not(target_env = "musl")))]
#[test]
fn vm_boot_args_full_roundtrip() {
    #[derive(Parser)]
    struct Wrapper {
        #[command(flatten)]
        args: VmBootArgs,
    }

    let w = Wrapper::parse_from([
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
        "4096",
        "--mount",
        "/host/src:/guest/src",
        "--mount",
        "/host/data:/guest/data",
        "--env",
        "FOO=bar",
        "--env",
        "BAZ=qux",
        "--arg",
        "server.js",
        "--arg",
        "--port",
        "--arg",
        "3000",
        "--pid-file",
        "/tmp/worker.pid",
        "--console-output",
        "/tmp/console.log",
        "--slot",
        "42",
    ]);

    assert_eq!(w.args.rootfs, "/tmp/rootfs");
    assert_eq!(w.args.exec, "/usr/bin/node");
    assert_eq!(w.args.workdir, "/workspace");
    assert_eq!(w.args.vcpus, 4);
    assert_eq!(w.args.ram, 4096);
    assert_eq!(
        w.args.mount,
        vec!["/host/src:/guest/src", "/host/data:/guest/data"]
    );
    assert_eq!(w.args.env, vec!["FOO=bar", "BAZ=qux"]);
    assert_eq!(w.args.arg, vec!["server.js", "--port", "3000"]);
    assert_eq!(w.args.pid_file, Some("/tmp/worker.pid".to_string()));
    assert_eq!(w.args.console_output, Some("/tmp/console.log".to_string()));
    assert_eq!(w.args.slot, 42);
}

/// `VmBootArgs` applies correct defaults for optional fields.
#[cfg(all(target_os = "linux", not(target_env = "musl")))]
#[test]
fn vm_boot_args_defaults() {
    #[derive(Parser)]
    struct Wrapper {
        #[command(flatten)]
        args: VmBootArgs,
    }

    let w = Wrapper::parse_from(["test", "--rootfs", "/tmp/rootfs", "--exec", "/usr/bin/node"]);
    assert_eq!(w.args.workdir, "/");
    assert_eq!(w.args.vcpus, 2);
    assert_eq!(w.args.ram, 2048);
    assert!(w.args.mount.is_empty());
    assert!(w.args.env.is_empty());
    assert!(w.args.arg.is_empty());
    assert!(w.args.pid_file.is_none());
    assert!(w.args.console_output.is_none());
    assert_eq!(w.args.slot, 0);
}

/// Manifest YAML roundtrip (serde pattern test, kept as-is).
#[test]
fn manifest_yaml_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let yaml = r#"
name: integration-test-worker
runtime:
  language: typescript
  package_manager: npm
  entry: src/index.ts
env:
  NODE_ENV: production
  API_KEY: test-key
resources:
  cpus: 4
  memory: 4096
"#;
    std::fs::write(dir.path().join("iii.worker.yaml"), yaml).unwrap();

    let content = std::fs::read_to_string(dir.path().join("iii.worker.yaml")).unwrap();
    let parsed: serde_yaml::Value = serde_yaml::from_str(&content).unwrap();

    assert_eq!(parsed["name"].as_str(), Some("integration-test-worker"));
    assert_eq!(parsed["runtime"]["language"].as_str(), Some("typescript"));
    assert_eq!(parsed["runtime"]["package_manager"].as_str(), Some("npm"));
    assert_eq!(parsed["env"]["NODE_ENV"].as_str(), Some("production"));
    assert_eq!(parsed["resources"]["cpus"].as_u64(), Some(4));
    assert_eq!(parsed["resources"]["memory"].as_u64(), Some(4096));
}

/// OCI config JSON parsing (serde pattern test, kept as-is).
#[test]
fn oci_config_json_parsing() {
    let dir = tempfile::tempdir().unwrap();
    let config = serde_json::json!({
        "config": {
            "Entrypoint": ["/usr/bin/node"],
            "Cmd": ["server.js", "--port", "8080"],
            "Env": [
                "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
                "NODE_VERSION=20.11.0",
                "HOME=/root"
            ]
        }
    });
    std::fs::write(
        dir.path().join(".oci-config.json"),
        serde_json::to_string_pretty(&config).unwrap(),
    )
    .unwrap();

    let content = std::fs::read_to_string(dir.path().join(".oci-config.json")).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();

    let entrypoint = parsed["config"]["Entrypoint"].as_array().unwrap();
    assert_eq!(entrypoint[0].as_str(), Some("/usr/bin/node"));

    let cmd = parsed["config"]["Cmd"].as_array().unwrap();
    assert_eq!(cmd.len(), 3);

    let env = parsed["config"]["Env"].as_array().unwrap();
    assert_eq!(env.len(), 3);
}
