//! Integration tests for iii-worker.
//!
//! These tests import the real `Cli`, `Commands`, and `VmBootArgs` types from
//! the crate library, ensuring any CLI changes are caught at compile time.

use clap::Parser;
use iii_worker::{Cli, Commands, VmBootArgs};

/// All 10 subcommands parse without error.
#[test]
fn cli_parses_all_subcommands() {
    let cases: Vec<(&[&str], fn(Commands))> = vec![
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
        (
            &[
                "iii-worker",
                "__vm-boot",
                "--rootfs",
                "/tmp/rootfs",
                "--exec",
                "/usr/bin/node",
            ],
            |c| assert!(matches!(c, Commands::VmBoot(_))),
        ),
    ];

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
        Commands::Add { args, force } => {
            assert_eq!(
                args.worker_names,
                vec!["ghcr.io/iii-hq/node:latest".to_string()]
            );
            assert!(!force);
        }
        _ => panic!("expected Add"),
    }
}

/// `add` subcommand accepts multiple worker names as positional args.
#[test]
fn add_subcommand_multiple_workers() {
    let cli = Cli::parse_from(["iii-worker", "add", "pdfkit", "iii-http", "iii-state"]);
    match cli.command {
        Commands::Add { args, force } => {
            assert_eq!(args.worker_names, vec!["pdfkit", "iii-http", "iii-state"]);
            assert!(!force);
        }
        _ => panic!("Expected Add command"),
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

/// `add --force` parses the force flag correctly.
#[test]
fn add_force_flag() {
    let cli = Cli::parse_from(["iii-worker", "add", "pdfkit", "--force"]);
    match cli.command {
        Commands::Add { args, force } => {
            assert_eq!(args.worker_names, vec!["pdfkit"]);
            assert!(force);
            assert!(!args.reset_config);
        }
        _ => panic!("expected Add"),
    }
}

/// `add --force --reset-config` parses both flags.
#[test]
fn add_force_reset_config() {
    let cli = Cli::parse_from(["iii-worker", "add", "pdfkit", "--force", "--reset-config"]);
    match cli.command {
        Commands::Add { args, force } => {
            assert!(force);
            assert!(args.reset_config);
        }
        _ => panic!("expected Add"),
    }
}

/// `add -f` short flag works.
#[test]
fn add_force_short_flag() {
    let cli = Cli::parse_from(["iii-worker", "add", "pdfkit", "-f"]);
    match cli.command {
        Commands::Add { force, .. } => assert!(force),
        _ => panic!("expected Add"),
    }
}

/// `reinstall` parses as expected and shares AddArgs with Add.
#[test]
fn reinstall_subcommand() {
    let cli = Cli::parse_from(["iii-worker", "reinstall", "pdfkit@1.2.0"]);
    match cli.command {
        Commands::Reinstall { args } => {
            assert_eq!(args.worker_names, vec!["pdfkit@1.2.0"]);
            assert!(!args.reset_config);
        }
        _ => panic!("expected Reinstall"),
    }
}

/// `reinstall --reset-config` parses the flag.
#[test]
fn reinstall_reset_config() {
    let cli = Cli::parse_from(["iii-worker", "reinstall", "pdfkit", "--reset-config"]);
    match cli.command {
        Commands::Reinstall { args } => {
            assert!(args.reset_config);
        }
        _ => panic!("expected Reinstall"),
    }
}

/// `clear` without args parses as clear-all.
#[test]
fn clear_subcommand_no_args() {
    let cli = Cli::parse_from(["iii-worker", "clear"]);
    match cli.command {
        Commands::Clear { worker_name, yes } => {
            assert!(worker_name.is_none());
            assert!(!yes);
        }
        _ => panic!("expected Clear"),
    }
}

/// `clear <name>` parses the worker name.
#[test]
fn clear_subcommand_with_name() {
    let cli = Cli::parse_from(["iii-worker", "clear", "pdfkit"]);
    match cli.command {
        Commands::Clear { worker_name, yes } => {
            assert_eq!(worker_name.as_deref(), Some("pdfkit"));
            assert!(!yes);
        }
        _ => panic!("expected Clear"),
    }
}

/// `clear --yes` / `clear -y` skips confirmation.
#[test]
fn clear_yes_flag() {
    let cli = Cli::parse_from(["iii-worker", "clear", "--yes"]);
    match cli.command {
        Commands::Clear { yes, .. } => assert!(yes),
        _ => panic!("expected Clear"),
    }
    let cli = Cli::parse_from(["iii-worker", "clear", "-y"]);
    match cli.command {
        Commands::Clear { yes, .. } => assert!(yes),
        _ => panic!("expected Clear"),
    }
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
