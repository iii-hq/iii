// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use clap::{CommandFactory, FromArgMatches};
use iii_worker::{Cli, Commands};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();

    // The `iii` dispatcher routes `iii sandbox ...` here, but our root
    // bin_name is "iii worker" so clap renders `Usage: iii worker sandbox`.
    // Peek at argv: if the first non-flag arg is `sandbox`, override the
    // root bin_name to `iii` for that one invocation. Per-subcommand bin_name
    // overrides don't fix it — clap's help walker always uses the root's.
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    let is_sandbox = args
        .iter()
        .skip(1)
        .find(|a| !a.to_string_lossy().starts_with('-'))
        .map_or(false, |a| a == "sandbox");

    let mut cmd = Cli::command();
    if is_sandbox {
        cmd = cmd.bin_name("iii");
    }
    let matches = cmd.get_matches_from(&args);
    let cli_args =
        Cli::from_arg_matches(&matches).map_err(|e| anyhow::anyhow!("cli parse: {e}"))?;

    let exit_code = match cli_args.command {
        Commands::Add {
            args,
            force,
            no_wait,
        } => {
            use iii_worker::cli::host_shim::CliHostShim;
            use iii_worker::cli::stderr_sink::StderrSink;
            use iii_worker::core::{AddOptions, ProjectCtx, add as core_add};

            let total = args.worker_names.len();
            let brief = total > 1;
            let mut fail_count = 0usize;

            for (i, name) in args.worker_names.iter().enumerate() {
                if brief {
                    use colored::Colorize;
                    eprintln!("  [{}/{}] Adding {}...", i + 1, total, name.bold());
                }

                let opts = AddOptions {
                    source: parse_source_for_cli(name),
                    force,
                    reset_config: args.reset_config,
                    wait: !no_wait,
                };

                let cwd = match std::env::current_dir() {
                    Ok(p) => p,
                    Err(e) => {
                        eprintln!("error: cannot resolve project root: {e}");
                        fail_count += 1;
                        continue;
                    }
                };
                let ctx = ProjectCtx::open_unlocked(cwd);
                let sink = StderrSink::new(brief);
                let result =
                    core_add::run(opts, &ctx, &sink, &CliHostShim, core_add::CallerMode::Cli).await;

                if let Err(e) = result {
                    eprintln!("error: [{}] {}", e.kind().code(), e);
                    fail_count += 1;
                }
            }

            if fail_count == 0 { 0 } else { 1 }
        }
        Commands::Remove { worker_names, yes } => {
            use iii_worker::cli::host_shim::CliHostShim;
            use iii_worker::cli::stderr_sink::StderrSink;
            use iii_worker::core::{ProjectCtx, RemoveOptions, remove as core_remove};
            use std::io::IsTerminal;

            // Consent: -y / --yes always proceeds. Without -y, prompt when
            // stderr is a tty; refuse non-interactively so scripts don't
            // silently remove workers without confirmation.
            let confirmed = if yes {
                true
            } else if std::io::stderr().is_terminal() {
                use std::io::{BufRead, Write};
                let names_str = worker_names.join(", ");
                eprint!("Remove worker(s) '{}'? [y/N] ", names_str);
                let _ = std::io::stderr().flush();
                let mut buf = String::new();
                let _ = std::io::stdin().lock().read_line(&mut buf);
                let trimmed = buf.trim().to_lowercase();
                trimmed == "y" || trimmed == "yes"
            } else {
                eprintln!("error: remove is destructive; pass -y/--yes for non-interactive use");
                std::process::exit(1);
            };
            if !confirmed {
                eprintln!("Aborted.");
                std::process::exit(1);
            }
            // `all` is unused on the CLI surface (users list names explicitly).
            let opts = RemoveOptions {
                names: worker_names,
                all: false,
                yes: true,
            };
            let cwd = std::env::current_dir().unwrap_or_else(|e| {
                eprintln!("error: cannot resolve project root: {e}");
                std::process::exit(1);
            });
            let ctx = ProjectCtx::open_unlocked(cwd);
            let sink = StderrSink::new(false);
            match core_remove::run(opts, &ctx, &sink, &CliHostShim).await {
                Ok(_) => 0,
                Err(e) => {
                    eprintln!("error: [{}] {}", e.kind().code(), e);
                    1
                }
            }
        }
        Commands::Reinstall { args } => {
            use iii_worker::cli::host_shim::CliHostShim;
            use iii_worker::cli::stderr_sink::StderrSink;
            use iii_worker::core::{AddOptions, ProjectCtx, add as core_add};

            let mut fail_count = 0usize;
            for name in &args.worker_names {
                let opts = AddOptions {
                    source: parse_source_for_cli(name),
                    force: true,
                    reset_config: args.reset_config,
                    wait: false,
                };

                let cwd = match std::env::current_dir() {
                    Ok(p) => p,
                    Err(e) => {
                        eprintln!("error: cannot resolve project root: {e}");
                        fail_count += 1;
                        continue;
                    }
                };
                let ctx = ProjectCtx::open_unlocked(cwd);
                let sink = StderrSink::new(false);
                if let Err(e) =
                    core_add::run(opts, &ctx, &sink, &CliHostShim, core_add::CallerMode::Cli).await
                {
                    eprintln!("error: [{}] {}", e.kind().code(), e);
                    fail_count += 1;
                }
            }
            if fail_count == 0 { 0 } else { 1 }
        }
        Commands::Update { worker_name } => {
            use iii_worker::cli::host_shim::CliHostShim;
            use iii_worker::cli::stderr_sink::StderrSink;
            use iii_worker::core::{ProjectCtx, UpdateOptions, update as core_update};

            let opts = UpdateOptions {
                names: worker_name.map(|n| vec![n]).unwrap_or_default(),
            };
            let cwd = std::env::current_dir().unwrap_or_else(|e| {
                eprintln!("error: cannot resolve project root: {e}");
                std::process::exit(1);
            });
            let ctx = ProjectCtx::open_unlocked(cwd);
            let sink = StderrSink::new(false);
            match core_update::run(opts, &ctx, &sink, &CliHostShim).await {
                Ok(_) => 0,
                Err(e) => {
                    eprintln!("error: [{}] {}", e.kind().code(), e);
                    1
                }
            }
        }
        Commands::Clear { worker_name, yes } => {
            use iii_worker::cli::host_shim::CliHostShim;
            use iii_worker::cli::stderr_sink::StderrSink;
            use iii_worker::core::{ClearOptions, ProjectCtx, clear as core_clear};
            use std::io::IsTerminal;

            // Map the existing CLI shape onto the new symmetric schema:
            // no name = wipe all, with-name = wipe just that one.
            let (names, all) = match worker_name {
                Some(n) => (vec![n], false),
                None => (Vec::new(), true),
            };
            // Consent: -y / --yes always proceeds. Without -y, prompt when
            // stderr is a tty; refuse non-interactively so scripts don't
            // silently wipe artifacts without confirmation.
            let target_label = if all {
                "all worker artifacts".to_string()
            } else {
                format!("artifacts for '{}'", names.join(", "))
            };
            let confirmed = if yes {
                true
            } else if std::io::stderr().is_terminal() {
                use std::io::{BufRead, Write};
                eprint!("Clear {}? [y/N] ", target_label);
                let _ = std::io::stderr().flush();
                let mut buf = String::new();
                let _ = std::io::stdin().lock().read_line(&mut buf);
                let trimmed = buf.trim().to_lowercase();
                trimmed == "y" || trimmed == "yes"
            } else {
                eprintln!("error: clear is destructive; pass -y/--yes for non-interactive use");
                std::process::exit(1);
            };
            if !confirmed {
                eprintln!("Aborted.");
                std::process::exit(1);
            }
            let opts = ClearOptions {
                names,
                all,
                yes: true,
            };
            let cwd = std::env::current_dir().unwrap_or_else(|e| {
                eprintln!("error: cannot resolve project root: {e}");
                std::process::exit(1);
            });
            let ctx = ProjectCtx::open_unlocked(cwd);
            let sink = StderrSink::new(false);
            match core_clear::run(opts, &ctx, &sink, &CliHostShim).await {
                Ok(_) => 0,
                Err(e) => {
                    eprintln!("error: [{}] {}", e.kind().code(), e);
                    1
                }
            }
        }
        Commands::Start {
            worker_name,
            no_wait,
            port,
            config,
        } => {
            use iii_worker::cli::host_shim::CliHostShim;
            use iii_worker::cli::stderr_sink::StderrSink;
            use iii_worker::core::{ProjectCtx, StartOptions, start as core_start};

            // Adapt CLI Start arg shape to StartOptions.
            let opts = StartOptions {
                name: worker_name,
                port: Some(port),
                config: config.map(|p| p.display().to_string()),
                wait: !no_wait,
            };
            let cwd = std::env::current_dir().unwrap_or_else(|e| {
                eprintln!("error: cannot resolve project root: {e}");
                std::process::exit(1);
            });
            let ctx = ProjectCtx::open_unlocked(cwd);
            let sink = StderrSink::new(false);
            match core_start::run(opts, &ctx, &sink, &CliHostShim).await {
                Ok(_) => 0,
                Err(e) => {
                    eprintln!("error: [{}] {}", e.kind().code(), e);
                    1
                }
            }
        }
        Commands::Stop { worker_name, yes } => {
            use iii_worker::cli::host_shim::CliHostShim;
            use iii_worker::cli::stderr_sink::StderrSink;
            use iii_worker::core::{ProjectCtx, StopOptions, stop as core_stop};
            use std::io::IsTerminal;

            // Consent: -y / --yes always proceeds. Without -y, prompt
            // when stderr is a tty; refuse non-interactively so scripts
            // don't silently stop workers without confirmation.
            if !yes {
                if std::io::stderr().is_terminal() {
                    use std::io::{BufRead, Write};
                    eprint!("Stop worker '{}'? [y/N] ", worker_name);
                    let _ = std::io::stderr().flush();
                    let mut buf = String::new();
                    let _ = std::io::stdin().lock().read_line(&mut buf);
                    let trimmed = buf.trim().to_lowercase();
                    if trimmed != "y" && trimmed != "yes" {
                        eprintln!("Aborted.");
                        std::process::exit(1);
                    }
                } else {
                    eprintln!("error: stop is destructive; pass -y/--yes for non-interactive use");
                    std::process::exit(1);
                }
            }
            let opts = StopOptions {
                name: worker_name,
                yes: true,
            };
            let cwd = std::env::current_dir().unwrap_or_else(|e| {
                eprintln!("error: cannot resolve project root: {e}");
                std::process::exit(1);
            });
            let ctx = ProjectCtx::open_unlocked(cwd);
            let sink = StderrSink::new(false);
            match core_stop::run(opts, &ctx, &sink, &CliHostShim).await {
                Ok(_) => 0,
                Err(e) => {
                    eprintln!("error: [{}] {}", e.kind().code(), e);
                    1
                }
            }
        }
        Commands::Restart {
            worker_name,
            no_wait,
            port,
            config,
        } => {
            iii_worker::cli::managed::handle_managed_restart(
                &worker_name,
                !no_wait,
                port,
                config.as_deref(),
            )
            .await
        }
        Commands::List => {
            use colored::Colorize;
            use iii_worker::cli::host_shim::CliHostShim;
            use iii_worker::cli::stderr_sink::StderrSink;
            use iii_worker::core::{ListOptions, ProjectCtx, list as core_list};

            let opts = ListOptions::default();
            let cwd = std::env::current_dir().unwrap_or_else(|e| {
                eprintln!("error: cannot resolve project root: {e}");
                std::process::exit(1);
            });
            let ctx = ProjectCtx::open_unlocked(cwd);
            let sink = StderrSink::new(false);
            match core_list::run(opts, &ctx, &sink, &CliHostShim).await {
                Ok(outcome) => {
                    if outcome.workers.is_empty() {
                        eprintln!("  No workers. Use `iii worker add` to get started.");
                    } else {
                        eprintln!();
                        eprintln!(
                            "  {:25} {:10} {:10} {}",
                            "NAME".bold(),
                            "VERSION".bold(),
                            "PID".bold(),
                            "STATUS".bold(),
                        );
                        eprintln!(
                            "  {:25} {:10} {:10} {}",
                            "----".dimmed(),
                            "-------".dimmed(),
                            "---".dimmed(),
                            "------".dimmed(),
                        );
                        for w in &outcome.workers {
                            let version = w.version.clone().unwrap_or_else(|| "-".to_string());
                            let pid = w
                                .pid
                                .map(|p| p.to_string())
                                .unwrap_or_else(|| "-".to_string());
                            let status = if w.running {
                                "running".green().to_string()
                            } else {
                                "stopped".dimmed().to_string()
                            };
                            eprintln!(
                                "  {:25} {:10} {:10} {}",
                                w.name,
                                version.dimmed(),
                                pid.dimmed(),
                                status
                            );
                        }
                        eprintln!();
                    }
                    0
                }
                Err(e) => {
                    eprintln!("error: [{}] {}", e.kind().code(), e);
                    1
                }
            }
        }
        Commands::Sync { frozen } => iii_worker::cli::managed::handle_worker_sync(frozen).await,
        Commands::Verify { strict } => iii_worker::cli::managed::handle_worker_verify(strict).await,
        Commands::Status {
            worker_name,
            no_watch,
        } => iii_worker::cli::status::handle_worker_status(&worker_name, !no_watch).await,
        Commands::Logs {
            worker_name,
            follow,
            address,
            port,
        } => {
            iii_worker::cli::managed::handle_managed_logs(&worker_name, follow, &address, port)
                .await
        }
        Commands::Exec(args) => {
            let handler = iii_worker::cli::shell_client::handle_managed_exec;
            handler(args).await
        }
        Commands::Sandbox { cmd } => match cmd {
            iii_worker::cli::app::SandboxCmd::Run {
                image,
                cpus,
                memory,
                port,
                cmd,
            } => iii_worker::cli::sandbox::handle_run(image, cmd, cpus, memory, port).await,
            iii_worker::cli::app::SandboxCmd::Create {
                image,
                cpus,
                memory,
                idle_timeout,
                name,
                network,
                env,
                port,
            } => {
                iii_worker::cli::sandbox::handle_create(
                    image,
                    cpus,
                    memory,
                    idle_timeout,
                    name,
                    network,
                    env,
                    port,
                )
                .await
            }
            iii_worker::cli::app::SandboxCmd::Exec {
                id,
                timeout,
                env,
                port,
                cmd,
            } => iii_worker::cli::sandbox::handle_exec(id, timeout, env, port, cmd).await,
            iii_worker::cli::app::SandboxCmd::List { all, port } => {
                iii_worker::cli::sandbox::handle_list(all, port).await
            }
            iii_worker::cli::app::SandboxCmd::Stop { id, port } => {
                iii_worker::cli::sandbox::handle_stop(id, port).await
            }
            iii_worker::cli::app::SandboxCmd::Upload {
                id,
                local_path,
                remote_path,
                mode,
                parents,
                port,
            } => {
                iii_worker::cli::sandbox::handle_upload(
                    id,
                    local_path,
                    remote_path,
                    mode,
                    parents,
                    port,
                )
                .await
            }
            iii_worker::cli::app::SandboxCmd::Download {
                id,
                remote_path,
                local_path,
                port,
            } => iii_worker::cli::sandbox::handle_download(id, remote_path, local_path, port).await,
        },
        Commands::SandboxDaemon(args) => iii_worker::cli::sandbox_daemon::run(args).await,
        Commands::WorkerManagerDaemon(args) => {
            iii_worker::cli::worker_manager_daemon::run(args).await
        }
        Commands::VmBoot(args) => {
            // Run the VM on a dedicated OS thread: `msb_krun`'s virtio-blk
            // Drop impls call `tokio::Runtime::block_on` for async shutdown,
            // which panics inside our `#[tokio::main]` runtime. The std
            // thread gives those drops a runtime-free context.
            //
            // `vm_boot::run` is `-> !`; a clean join is a bug, exit 1.
            //
            // TODO(msb_krun upstream): remove this std::thread dispatch
            // once virtio-blk Drop uses `Handle::try_current()` instead
            // of unconditional `block_on`. Draft issue at
            // ~/.claude/plans/msb_krun-upstream-issue-draft.md — file via
            // `gh issue create` against microsandbox/microsandbox.
            let handle = std::thread::Builder::new()
                .name("iii-worker-vm-boot".to_string())
                .spawn(move || iii_worker::cli::vm_boot::run(&args))
                .expect("failed to spawn vm-boot thread");
            match handle.join() {
                Err(_) => {
                    eprintln!("error: vm-boot thread panicked");
                    std::process::exit(1);
                }
                Ok(_never) => {
                    eprintln!("error: vm-boot returned without std::process::exit");
                    std::process::exit(1);
                }
            }
        }
        Commands::WatchSource(args) => {
            let project = std::path::PathBuf::from(&args.project);
            let worker = args.worker.clone();
            iii_worker::cli::source_watcher::watch_and_restart(
                worker,
                project,
                iii_worker::cli::source_watcher::restart_via_cli,
            )
            .await?;
            0
        }
    };

    std::process::exit(exit_code);
}

fn parse_source_for_cli(input: &str) -> iii_worker::core::WorkerSource {
    if iii_worker::cli::local_worker::is_local_path(input) {
        return iii_worker::core::WorkerSource::Local { path: input.into() };
    }
    // OCI ref heuristic: contains '/' OR contains ':' but not '@'
    // (`pdfkit@1.0` is registry name+version, not OCI).
    if input.contains('/') || (input.contains(':') && !input.contains('@')) {
        return iii_worker::core::WorkerSource::Oci {
            reference: input.into(),
        };
    }
    let (name, version) = match input.split_once('@') {
        Some((n, v)) => (n.to_string(), Some(v.to_string())),
        None => (input.to_string(), None),
    };
    iii_worker::core::WorkerSource::Registry { name, version }
}
