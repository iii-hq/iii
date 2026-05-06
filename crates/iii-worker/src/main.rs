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

    // The `sandbox` subtree is exposed through the `iii` dispatcher as
    // `iii sandbox ...`. When users run `iii sandbox --help`, clap would
    // otherwise show `Usage: iii worker sandbox <COMMAND>` because the
    // top-level bin_name is "iii worker". Clap's help generator always
    // walks from the root and uses the root's bin_name as the usage
    // prefix, so per-subcommand overrides don't change the displayed
    // path. Workaround: peek at argv and, if the first non-program
    // argument is `sandbox`, swap the root bin_name to `iii` so the
    // usage line reads `iii sandbox ...`. All other subcommands keep
    // the existing `iii worker <cmd>` usage.
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
            let wait = !no_wait;
            if force {
                let mut fail_count = 0;
                for name in &args.worker_names {
                    let result = iii_worker::cli::managed::handle_managed_add(
                        name,
                        false,
                        force,
                        args.reset_config,
                        wait,
                    )
                    .await;
                    if result != 0 {
                        fail_count += 1;
                    }
                }
                if fail_count == 0 { 0 } else { 1 }
            } else {
                iii_worker::cli::managed::handle_managed_add_many(&args.worker_names, wait).await
            }
        }
        Commands::Remove { worker_names, yes } => {
            iii_worker::cli::managed::handle_managed_remove_many(&worker_names, yes).await
        }
        Commands::Reinstall { args } => {
            let mut fail_count = 0;
            for name in &args.worker_names {
                let result = iii_worker::cli::managed::handle_managed_add(
                    name,
                    false,
                    true,
                    args.reset_config,
                    false,
                )
                .await;
                if result != 0 {
                    fail_count += 1;
                }
            }
            if fail_count == 0 { 0 } else { 1 }
        }
        Commands::Update { worker_name } => {
            iii_worker::cli::managed::handle_worker_update(worker_name.as_deref()).await
        }
        Commands::Clear { worker_name, yes } => {
            iii_worker::cli::managed::handle_managed_clear(worker_name.as_deref(), yes)
        }
        Commands::Start {
            worker_name,
            no_wait,
            port,
            config,
        } => {
            iii_worker::cli::managed::handle_managed_start(
                &worker_name,
                !no_wait,
                port,
                config.as_deref(),
            )
            .await
        }
        Commands::Stop { worker_name } => {
            iii_worker::cli::managed::handle_managed_stop(&worker_name).await
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
        Commands::List => iii_worker::cli::managed::handle_worker_list().await,
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
        Commands::VmBoot(args) => {
            // Run the VM on a dedicated OS thread. `msb_krun`'s virtio-blk
            // devices (imago-backed) call `tokio::Runtime::block_on` in
            // their Drop impl to finalize async storage shutdown. Nested
            // block_on inside our outer `#[tokio::main]` runtime panics
            // with "Cannot start a runtime from within a runtime" — only
            // observable once a `.disk()` attach is wired in. Spawning
            // on a std::thread gives the Drop impl a runtime-free
            // context so it can construct its own ephemeral runtime
            // without tripping tokio's nested-runtime detector.
            //
            // `vm_boot::run` has return type `-> !` so the thread only
            // returns on panic; normal exit happens via
            // `std::process::exit`. If we ever see the join below
            // return cleanly, something has returned from `run` that
            // was supposed to diverge — treat it as a bug, exit 1.
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
