// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

pub mod advisory;
pub mod download;
pub mod error;
pub mod exec;
pub mod github;
pub mod managed_shim;
pub mod platform;
pub mod registry;
pub mod state;
pub mod telemetry;
pub mod update;

use colored::Colorize;

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
        tracing::debug!(binary = %existing.display(), name = spec.name, "found existing binary");
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
