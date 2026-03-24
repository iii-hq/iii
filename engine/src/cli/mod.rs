pub mod advisory;
pub mod download;
pub mod error;
pub mod exec;
pub mod github;
pub mod platform;
pub mod registry;
pub mod state;
pub mod telemetry;
pub mod update;
pub mod worker_manager;

use colored::Colorize;
use error::WorkerError;

/// Handle dispatching a command to a managed binary.
pub async fn handle_dispatch(command: &str, args: &[String], no_update_check: bool) -> i32 {
    // Resolve command to binary spec
    let (spec, binary_subcommand) = match registry::resolve_command(command) {
        Ok(result) => result,
        Err(e) => {
            eprintln!("{} {}", "error:".red(), e);
            return 1;
        }
    };

    // Check platform support early
    if let Err(e) = platform::check_platform_support(spec) {
        eprintln!("{} {}", "error:".red(), e);
        return 1;
    }

    // Ensure storage directories exist
    if let Err(e) = platform::ensure_dirs() {
        eprintln!("{} {}", "error:".red(), e);
        return 1;
    }

    // Load state
    let mut app_state = match state::AppState::load(&platform::state_file_path()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{} Failed to load state: {}", "warning:".yellow(), e);
            state::AppState::default()
        }
    };

    // Resolve the binary path: check managed dir, then existing installations, then download
    let binary_path = if platform::binary_path(spec.name).exists() {
        platform::binary_path(spec.name)
    } else if let Some(existing) = platform::find_existing_binary(spec.name) {
        eprintln!(
            "  {} Found existing {} at {}",
            "✓".green(),
            spec.name,
            existing.display().to_string().dimmed()
        );
        existing
    } else {
        // Auto-download if binary is not present anywhere
        let managed_path = platform::binary_path(spec.name);
        eprintln!("  Retrieving dependencies for {}...", command.bold());

        let client = match github::build_client() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("{} Failed to create HTTP client: {}", "error:".red(), e);
                return 1;
            }
        };

        let release = match github::fetch_latest_release(&client, spec).await {
            Ok(r) => r,
            Err(e) => {
                eprintln!("{} {}", "error:".red(), e);
                return 1;
            }
        };

        let asset_name = platform::asset_name(spec.name);
        let asset = match github::find_asset(&release, &asset_name) {
            Some(a) => a,
            None => {
                eprintln!("{} Release asset not found: {}", "error:".red(), asset_name);
                return 1;
            }
        };

        let checksum_url = if spec.has_checksum {
            let checksum_name = platform::checksum_asset_name(spec.name);
            github::find_asset(&release, &checksum_name).map(|a| a.browser_download_url.clone())
        } else {
            None
        };

        if let Err(e) = download::download_and_install(
            &client,
            spec,
            asset,
            checksum_url.as_deref(),
            &managed_path,
        )
        .await
        {
            eprintln!("{} {}", "error:".red(), e);
            return 1;
        }

        // Record installation in state
        let version = github::parse_release_version(&release.tag_name)
            .unwrap_or_else(|_| semver::Version::new(0, 0, 0));
        app_state.record_install(spec.name, version, asset_name);
        let _ = app_state.save(&platform::state_file_path());

        eprintln!("  {} {} installed successfully", "✓".green(), spec.name);

        // Hint if ~/.local/bin is not on PATH
        #[cfg(not(target_os = "windows"))]
        {
            let path_var = std::env::var("PATH").unwrap_or_default();
            if !path_var.split(':').any(|p| p.ends_with(".local/bin")) {
                eprintln!(
                    "  {} add {} to your PATH to run {} directly",
                    "hint:".dimmed(),
                    "~/.local/bin".bold(),
                    spec.name
                );
            }
        }

        eprintln!();

        managed_path
    };

    // Run background update check (non-blocking, 500ms timeout)
    if !no_update_check {
        if let Some((updates, should_save)) = update::run_background_check(&app_state, 500).await {
            // Print update notifications
            update::print_update_notifications(&updates);

            // Check advisories too
            if let Ok(client) = github::build_client() {
                if let Ok(advisories) = advisory::fetch_advisories(&client).await {
                    let matched = advisory::check_advisories(&advisories, &app_state);
                    advisory::print_advisory_warnings(&matched);
                }
            }

            // Save updated state
            if should_save {
                app_state.mark_update_checked();
                let _ = app_state.save(&platform::state_file_path());
            }
        }
    }

    // Build args for the child binary
    let mut child_args: Vec<String> = Vec::new();
    if let Some(subcmd) = binary_subcommand {
        child_args.push(subcmd.to_string());
    }
    child_args.extend_from_slice(args);

    // Execute the binary (replaces process on Unix)
    match exec::run_binary(&binary_path, &child_args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{} {}", "error:".red(), e);
            1
        }
    }
}

/// Parse a worker argument that may contain an @version suffix.
/// Returns (name, optional_version). Rejects empty name or empty version.
fn parse_worker_arg(input: &str) -> Result<(&str, Option<&str>), WorkerError> {
    if let Some((name, version)) = input.rsplit_once('@') {
        if name.is_empty() {
            return Err(WorkerError::InvalidWorkerName {
                name: input.to_string(),
            });
        }
        if version.is_empty() {
            return Err(WorkerError::InvalidWorkerName {
                name: input.to_string(),
            });
        }
        Ok((name, Some(version)))
    } else {
        Ok((input, None))
    }
}

/// Handle the install command for a single worker.
async fn handle_install_single(worker_name: &str, version: Option<&str>, force: bool) -> i32 {
    let client = match github::build_client() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{} Failed to create HTTP client: {}", "error:".red(), e);
            return 1;
        }
    };

    // Use current directory as project root
    let project_dir = match std::env::current_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!(
                "{} Failed to determine current directory: {}",
                "error:".red(),
                e
            );
            return 1;
        }
    };

    let version_display = version.map(|v| format!("@{}", v)).unwrap_or_default();
    eprintln!(
        "  Installing worker {}{}...",
        worker_name.bold(),
        version_display
    );

    match worker_manager::install::install_worker(worker_name, version, &project_dir, &client, force)
        .await
    {
        Ok(worker_manager::install::InstallOutcome::Installed {
            name,
            version: ver,
            config_updated,
        }) => {
            eprintln!("  {} {} v{} installed successfully", "✓".green(), name, ver);
            if config_updated {
                eprintln!(
                    "  {} config.yaml updated with default configuration",
                    "✓".green()
                );
            }
            0
        }
        Ok(worker_manager::install::InstallOutcome::Updated {
            name,
            old_version,
            new_version,
            config_updated,
        }) => {
            eprintln!(
                "  {} {} updated {} -> {}",
                "✓".green(),
                name,
                old_version,
                new_version
            );
            if config_updated {
                eprintln!(
                    "  {} config.yaml updated with default configuration",
                    "✓".green()
                );
            }
            0
        }
        Err(e) => {
            eprintln!("{} {}", "error:".red(), e);
            1
        }
    }
}

/// Handle bulk install: read iii.toml and install all workers listed there.
async fn handle_install_all(force: bool) -> i32 {
    let project_dir = match std::env::current_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!(
                "{} Failed to determine current directory: {}",
                "error:".red(),
                e
            );
            return 1;
        }
    };

    let manifest = match worker_manager::manifest::read_manifest(&project_dir) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{} {}", "error:".red(), e);
            return 1;
        }
    };

    if manifest.is_empty() {
        eprintln!("  No workers defined in iii.toml. Nothing to install.");
        return 0;
    }

    let client = match github::build_client() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{} Failed to create HTTP client: {}", "error:".red(), e);
            return 1;
        }
    };

    eprintln!("  Installing {} worker(s) from iii.toml...", manifest.len());
    eprintln!();

    let mut failed = 0u32;
    let mut installed = 0u32;
    let mut up_to_date = 0u32;

    for (name, version) in &manifest {
        let binary_path = worker_manager::storage::worker_binary_path(&project_dir, name);
        if binary_path.exists() {
            let installed_version =
                worker_manager::storage::read_installed_version(&project_dir, name);
            if installed_version.as_deref() == Some(version.as_str()) {
                eprintln!(
                    "  {} {} v{} (already installed)",
                    "-".dimmed(),
                    name,
                    version
                );
                up_to_date += 1;
                continue;
            }
            eprintln!(
                "  Updating {} (installed: {}, manifest: v{})...",
                name.bold(),
                installed_version
                    .as_deref()
                    .map_or("unknown".to_string(), |v| format!("v{}", v)),
                version
            );
        } else {
            eprintln!("  Installing {}@{}...", name.bold(), version);
        }

        match worker_manager::install::install_worker(
            name,
            Some(version.as_str()),
            &project_dir,
            &client,
            force,
        )
        .await
        {
            Ok(worker_manager::install::InstallOutcome::Installed {
                name: n,
                version: v,
                config_updated,
            }) => {
                eprintln!("  {} {} v{} installed", "✓".green(), n, v);
                if config_updated {
                    eprintln!("  {} config.yaml updated", "✓".green());
                }
                installed += 1;
            }
            Ok(worker_manager::install::InstallOutcome::Updated {
                name: n,
                old_version,
                new_version,
                config_updated,
            }) => {
                eprintln!(
                    "  {} {} updated {} -> {}",
                    "✓".green(),
                    n,
                    old_version,
                    new_version
                );
                if config_updated {
                    eprintln!("  {} config.yaml updated", "✓".green());
                }
                installed += 1;
            }
            Err(e) => {
                eprintln!("  {} {} failed: {}", "✗".red(), name, e);
                failed += 1;
            }
        }
    }

    eprintln!();
    if failed > 0 {
        eprintln!(
            "  {} installed, {} up to date, {} failed",
            installed, up_to_date, failed
        );
        1
    } else {
        eprintln!("  {} installed, {} up to date", installed, up_to_date);
        0
    }
}

/// Handle the install command. Routes to single or bulk install.
pub async fn handle_install(worker_name: Option<&str>, force: bool) -> i32 {
    match worker_name {
        Some(name) => match parse_worker_arg(name) {
            Ok((name, version)) => handle_install_single(name, version, force).await,
            Err(e) => {
                eprintln!("{} {}", "error:".red(), e);
                1
            }
        },
        None => handle_install_all(force).await,
    }
}

/// Handle the uninstall command for workers.
pub fn handle_uninstall(worker_name: &str) -> i32 {
    let project_dir = match std::env::current_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!(
                "{} Failed to determine current directory: {}",
                "error:".red(),
                e
            );
            return 1;
        }
    };

    eprintln!("  Uninstalling worker {}...", worker_name.bold());

    match worker_manager::uninstall::uninstall_worker(worker_name, &project_dir) {
        Ok(outcome) => {
            if outcome.binary_removed {
                eprintln!("  {} Removed binary", "✓".green());
            } else {
                eprintln!("  {} Binary already absent", "-".dimmed());
            }
            eprintln!("  {} Removed from iii.toml", "✓".green());
            if outcome.config_removed {
                eprintln!("  {} Removed config.yaml block", "✓".green());
            } else {
                eprintln!("  {} No config.yaml block found", "-".dimmed());
            }
            for warning in &outcome.warnings {
                eprintln!("  {} {}", "warning:".yellow(), warning);
            }
            eprintln!(
                "  {} {} uninstalled successfully",
                "✓".green(),
                outcome.name
            );
            0
        }
        Err(e) => {
            eprintln!("{} {}", "error:".red(), e);
            1
        }
    }
}

/// Handle the update command.
pub async fn handle_update(target: Option<&str>) -> i32 {
    let client = match github::build_client() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{} Failed to create HTTP client: {}", "error:".red(), e);
            return 1;
        }
    };

    let mut app_state = match state::AppState::load(&platform::state_file_path()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{} Failed to load state: {}", "error:".red(), e);
            return 1;
        }
    };

    // Ensure storage directories exist
    if let Err(e) = platform::ensure_dirs() {
        eprintln!("{} {}", "error:".red(), e);
        return 1;
    }

    let results = match target {
        Some("iii" | "iii-cli" | "self") => {
            // Self-update only ("iii-cli" accepted for backward compat)
            vec![update::self_update(&client, &mut app_state).await]
        }
        Some(cmd) => {
            // Normalize SDK-namespaced commands to registry keys
            let registry_key = match cmd {
                "sdk" => "motia-cli",
                other => other,
            };
            // Update specific binary
            let spec = match registry::resolve_binary_for_update(registry_key) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("{} {}", "error:".red(), e);
                    return 1;
                }
            };
            vec![update::update_binary(&client, spec, &mut app_state).await]
        }
        None => {
            // Update all (includes self-update)
            eprintln!("  Checking all binaries for updates...");
            update::update_all(&client, &mut app_state).await
        }
    };

    // Print results
    let mut self_updated = false;
    for result in &results {
        update::print_update_result(result);
        if let Ok(update::UpdateResult::Updated { binary, .. }) = result {
            if binary == "iii" {
                self_updated = true;
            }
        }
    }

    // Print restart note after self-update
    if self_updated {
        eprintln!();
        eprintln!(
            "  {} iii has been updated. Restart your shell or run the command again to use the new version.",
            "note:".cyan(),
        );
    }

    // Save state
    app_state.mark_update_checked();
    if let Err(e) = app_state.save(&platform::state_file_path()) {
        eprintln!("{} Failed to save state: {}", "warning:".yellow(), e);
    }

    // Return non-zero if any update failed
    if results.iter().any(|r| r.is_err()) {
        1
    } else {
        0
    }
}

/// Handle the list command for workers (reads iii.toml).
pub fn handle_worker_list() -> i32 {
    let project_dir = match std::env::current_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!(
                "{} Failed to determine current directory: {}",
                "error:".red(),
                e
            );
            return 1;
        }
    };

    let workers = match worker_manager::manifest::read_manifest(&project_dir) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{} {}", "error:".red(), e);
            return 1;
        }
    };

    if workers.is_empty() {
        eprintln!("  No workers installed. Run `iii worker add <worker>` to get started.");
        return 0;
    }

    eprintln!();
    eprintln!("  {:20} {}", "WORKER".bold(), "VERSION".bold());
    eprintln!("  {:20} {}", "------".dimmed(), "-------".dimmed());
    for (name, version) in &workers {
        eprintln!("  {:20} {}", name, version);
    }
    eprintln!();
    0
}

/// Handle the info command for a worker (fetches registry + GitHub).
pub async fn handle_info(worker_name: &str) -> i32 {
    let client = match github::build_client() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{} Failed to create HTTP client: {}", "error:".red(), e);
            return 1;
        }
    };

    // Fetch registry
    let registry_manifest = match worker_manager::registry::fetch_registry(&client).await {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{} {}", "error:".red(), e);
            return 1;
        }
    };

    // Resolve worker
    let worker_entry = match registry_manifest.resolve(worker_name) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("{} {}", "error:".red(), e);
            return 1;
        }
    };

    // Build BinarySpec for fetch_latest_release
    let spec = worker_manager::spec::leaked_binary_spec(worker_name, worker_entry);

    // Fetch latest version from GitHub
    let version_display = match github::fetch_latest_release(&client, &spec).await {
        Ok(release) => match github::parse_release_version(&release.tag_name) {
            Ok(v) => format!("{}", v),
            Err(_) => release.tag_name.clone(),
        },
        Err(_) => "unknown".to_string(),
    };

    // Display info card
    eprintln!();
    eprintln!("  {}: {}", "Name".bold(), worker_name);
    eprintln!("  {}: {}", "Description".bold(), worker_entry.description);
    eprintln!("  {}: {}", "Latest version".bold(), version_display);
    eprintln!("  {}: {}", "Repository".bold(), worker_entry.repo);
    eprintln!(
        "  {}: {}",
        "Platforms".bold(),
        worker_entry.supported_targets.join(", ")
    );
    eprintln!(
        "  {}: {}",
        "Checksum verified".bold(),
        if worker_entry.has_checksum {
            "yes"
        } else {
            "no"
        }
    );
    eprintln!();
    0
}
