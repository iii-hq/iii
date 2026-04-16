//! Integration tests for iii-worker.
//!
//! These tests import the real `Cli`, `Commands`, and `VmBootArgs` types from
//! the crate library, ensuring any CLI changes are caught at compile time.

mod common;

use clap::Parser;
use common::isolation::in_temp_dir;
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

/// Engine/CLI IPC contract: the iii engine spawns `iii-worker start <name>
/// --port <N>` from engine/src/workers/registry_worker.rs::ExternalWorkerProcess::spawn
/// whenever it encounters a worker in config.yaml that isn't a builtin or a
/// legacy iii.toml module. The --port flag carries the engine's configured
/// iii-worker-manager port so spawned workers connect back to the right
/// place (previously hardcoded DEFAULT_PORT, silently breaking non-default
/// manager ports). These tests lock both halves of the contract: (a) the
/// bare-name form still parses for backward compat with direct CLI use, and
/// (b) the engine's new --port form parses and surfaces the port correctly.
#[test]
fn start_subcommand_matches_engine_spawn_args() {
    // Bare-name form used when a human runs `iii-worker start <name>` from
    // the terminal. Must still parse cleanly and default port to DEFAULT_PORT.
    // Humans DO expect the wait-for-ready status panel here; only the engine
    // auto-spawn path opts out via --no-wait.
    let cli = Cli::try_parse_from(["iii-worker", "start", "image-resize"])
        .expect("bare start form must parse");
    match cli.command {
        Commands::Start {
            worker_name,
            no_wait,
            port,
        } => {
            assert_eq!(worker_name, "image-resize");
            assert!(!no_wait, "bare human invocation keeps default wait=true");
            assert_eq!(
                port,
                iii_worker::DEFAULT_PORT,
                "bare form must default to DEFAULT_PORT"
            );
        }
        _ => panic!("expected Start"),
    }
}

#[test]
fn start_subcommand_accepts_port_flag_from_engine_spawn() {
    // Exact form the engine's ExternalWorkerProcess::spawn emits when a
    // non-default iii-worker-manager port is configured. If this ever stops
    // parsing, clap rejects with exit 2 and every auto-spawned external
    // worker on a non-default port silently fails to connect.
    //
    // `--no-wait` is part of that contract now: without it, the child blocks
    // on the 500ms status-panel redraw loop and floods stderr.log with ANSI
    // redraw noise that bleeds into `iii worker logs -f`.
    let cli = Cli::try_parse_from([
        "iii-worker",
        "start",
        "pdfkit",
        "--port",
        "49199",
        "--no-wait",
    ])
    .expect("engine's --port --no-wait spawn form must parse");
    match cli.command {
        Commands::Start {
            worker_name,
            no_wait,
            port,
        } => {
            assert_eq!(worker_name, "pdfkit");
            assert!(no_wait, "engine auto-spawn must pass --no-wait");
            assert_eq!(port, 49199, "--port must surface the custom port");
        }
        _ => panic!("expected Start"),
    }
}

#[test]
fn restart_subcommand_accepts_port_flag() {
    // Restart funnels through handle_managed_start too, so a CLI user who
    // runs `iii-worker restart foo --port 49199` against a non-default
    // engine must see the port flow through. Otherwise the same silent-fail
    // pattern returns via the restart path.
    let cli = Cli::try_parse_from(["iii-worker", "restart", "pdfkit", "--port", "49199"])
        .expect("restart --port must parse");
    match cli.command {
        Commands::Restart {
            worker_name, port, ..
        } => {
            assert_eq!(worker_name, "pdfkit");
            assert_eq!(port, 49199);
        }
        _ => panic!("expected Restart"),
    }
}

/// `add` subcommand parses worker name and applies defaults.
#[test]
fn add_subcommand_fields() {
    let cli = Cli::parse_from(["iii-worker", "add", "ghcr.io/iii-hq/node:latest"]);
    match cli.command {
        Commands::Add { args, force, .. } => {
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
        Commands::Add { args, force, .. } => {
            assert_eq!(args.worker_names, vec!["pdfkit", "iii-http", "iii-state"]);
            assert!(!force);
        }
        _ => panic!("Expected Add command"),
    }
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
    in_temp_dir(|| {
        let dir = std::env::current_dir().unwrap();
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
        std::fs::write(dir.join("iii.worker.yaml"), yaml).unwrap();

        let content = std::fs::read_to_string(dir.join("iii.worker.yaml")).unwrap();
        let parsed: serde_yaml::Value = serde_yaml::from_str(&content).unwrap();

        assert_eq!(parsed["name"].as_str(), Some("integration-test-worker"));
        assert_eq!(parsed["runtime"]["language"].as_str(), Some("typescript"));
        assert_eq!(parsed["runtime"]["package_manager"].as_str(), Some("npm"));
        assert_eq!(parsed["env"]["NODE_ENV"].as_str(), Some("production"));
        assert_eq!(parsed["resources"]["cpus"].as_u64(), Some(4));
        assert_eq!(parsed["resources"]["memory"].as_u64(), Some(4096));
    });
}

/// `add --force` parses the force flag correctly.
#[test]
fn add_force_flag() {
    let cli = Cli::parse_from(["iii-worker", "add", "pdfkit", "--force"]);
    match cli.command {
        Commands::Add { args, force, .. } => {
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
        Commands::Add { args, force, .. } => {
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

/// `add ./path` accepts relative local paths as worker names.
#[test]
fn add_subcommand_accepts_local_path() {
    let cli = Cli::parse_from(["iii-worker", "add", "./my-worker"]);
    match cli.command {
        Commands::Add { args, force, .. } => {
            assert_eq!(args.worker_names, vec!["./my-worker"]);
            assert!(!force);
        }
        _ => panic!("expected Add"),
    }
}

/// `add /absolute/path` accepts absolute local paths.
#[test]
fn add_subcommand_accepts_absolute_path() {
    let cli = Cli::parse_from(["iii-worker", "add", "/tmp/my-worker"]);
    match cli.command {
        Commands::Add { args, force, .. } => {
            assert_eq!(args.worker_names, vec!["/tmp/my-worker"]);
            assert!(!force);
        }
        _ => panic!("expected Add"),
    }
}

/// `add ./path --force` parses both path and force flag.
#[test]
fn add_subcommand_local_path_with_force() {
    let cli = Cli::parse_from(["iii-worker", "add", "./my-worker", "--force"]);
    match cli.command {
        Commands::Add { args, force, .. } => {
            assert_eq!(args.worker_names, vec!["./my-worker"]);
            assert!(force);
        }
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
    in_temp_dir(|| {
        let dir = std::env::current_dir().unwrap();
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
            dir.join(".oci-config.json"),
            serde_json::to_string_pretty(&config).unwrap(),
        )
        .unwrap();

        let content = std::fs::read_to_string(dir.join(".oci-config.json")).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();

        let entrypoint = parsed["config"]["Entrypoint"].as_array().unwrap();
        assert_eq!(entrypoint[0].as_str(), Some("/usr/bin/node"));

        let cmd = parsed["config"]["Cmd"].as_array().unwrap();
        assert_eq!(cmd.len(), 3);

        let env = parsed["config"]["Env"].as_array().unwrap();
        assert_eq!(env.len(), 3);
    });
}
