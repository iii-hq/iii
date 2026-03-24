// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::time::Duration;

use colored::Colorize;
use semver::Version;

use super::error::RegistryError;
use super::github::{self, IiiGithubError};
use super::registry::{self, BinarySpec};
use super::state::AppState;
use super::{download, platform, telemetry};

/// Information about an available update.
#[derive(Debug)]
pub struct UpdateInfo {
    pub binary_name: String,
    pub current_version: Version,
    pub latest_version: Version,
}

/// Check for updates for all installed binaries.
/// Returns a list of available updates.
pub async fn check_for_updates(client: &reqwest::Client, state: &AppState) -> Vec<UpdateInfo> {
    let mut updates = Vec::new();

    for (name, binary_state) in &state.binaries {
        // Find the spec for this binary
        let spec = match registry::all_binaries()
            .into_iter()
            .find(|s| s.name == name)
        {
            Some(s) => s,
            None => continue,
        };

        // Fetch latest release
        let release = match github::fetch_latest_release(client, spec).await {
            Ok(r) => r,
            Err(_) => continue, // Silently skip on error
        };

        // Parse version
        let latest = match github::parse_release_version(&release.tag_name) {
            Ok(v) => v,
            Err(_) => continue,
        };

        if latest > binary_state.version {
            updates.push(UpdateInfo {
                binary_name: name.clone(),
                current_version: binary_state.version.clone(),
                latest_version: latest,
            });
        }
    }

    updates
}

/// Print update notifications to stderr (informational, not prompting).
pub fn print_update_notifications(updates: &[UpdateInfo]) {
    if updates.is_empty() {
        return;
    }

    eprintln!();
    for update in updates {
        eprintln!(
            "  {} Update available: {} {} → {} (run `iii update {}`)",
            "info:".yellow(),
            update.binary_name,
            update.current_version.to_string().dimmed(),
            update.latest_version.to_string().green(),
            // Use the CLI command name, not the binary name
            cli_command_for_binary(&update.binary_name).unwrap_or(&update.binary_name),
        );
    }
    eprintln!();
}

/// Get the CLI command name for a binary name.
fn cli_command_for_binary(binary_name: &str) -> Option<&str> {
    for spec in registry::REGISTRY {
        if spec.name == binary_name {
            return spec.commands.first().map(|c| c.cli_command);
        }
    }
    None
}

/// Run the background update check with a bounded timeout.
/// Compatible with the process-replacement lifecycle.
///
/// Returns update notifications if the check completes within the timeout,
/// or None if it times out (will retry on next invocation).
pub async fn run_background_check(
    state: &AppState,
    timeout_ms: u64,
) -> Option<(Vec<UpdateInfo>, bool)> {
    if !state.is_update_check_due() {
        return None;
    }

    let client = match github::build_client() {
        Ok(c) => c,
        Err(_) => return None,
    };

    let check = async {
        let updates = check_for_updates(&client, state).await;
        (updates, true) // true = check completed, should update timestamp
    };

    match tokio::time::timeout(Duration::from_millis(timeout_ms), check).await {
        Ok(result) => Some(result),
        Err(_) => None, // Timed out, will retry next run
    }
}

/// Check if a managed binary is installed on disk.
fn is_binary_installed(name: &str) -> bool {
    platform::binary_path(name).exists() || platform::find_existing_binary(name).is_some()
}

/// Detect the actual version of an installed binary by running it with `--version`.
/// Returns None if the binary doesn't exist, can't be executed, or output can't be parsed.
fn detect_binary_version(name: &str) -> Option<Version> {
    let path = platform::binary_path(name);
    if !path.exists() {
        // Try PATH
        if let Some(p) = platform::find_existing_binary(name) {
            return run_version_check(&p);
        }
        return None;
    }
    run_version_check(&path)
}

fn run_version_check(path: &std::path::Path) -> Option<Version> {
    let output = std::process::Command::new(path)
        .arg("--version")
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Version could be just "0.8.0" or "iii 0.8.0" — take the last word
    let version_str = stdout
        .trim()
        .rsplit_once(' ')
        .map(|(_, v)| v)
        .unwrap_or(stdout.trim());
    Version::parse(version_str).ok()
}

/// Update a specific binary to the latest version.
pub async fn update_binary(
    client: &reqwest::Client,
    spec: &BinarySpec,
    state: &mut AppState,
) -> Result<UpdateResult, UpdateError> {
    // Check platform support
    platform::check_platform_support(spec)?;

    let binary_installed = is_binary_installed(spec.name);

    eprintln!("  Checking for updates to {}...", spec.name);

    // Fetch latest release
    let release = github::fetch_latest_release(client, spec).await?;
    let latest_version = github::parse_release_version(&release.tag_name)
        .map_err(|e| UpdateError::VersionParse(e.to_string()))?;

    // Check if already up to date using the actual on-disk binary version,
    // not the state file (which can be stale after manual installs or migrations).
    if binary_installed {
        if let Some(actual_version) = detect_binary_version(spec.name) {
            if actual_version >= latest_version {
                state.record_install(
                    spec.name,
                    actual_version.clone(),
                    platform::asset_name(spec.name),
                );
                return Ok(UpdateResult::AlreadyUpToDate {
                    binary: spec.name.to_string(),
                    version: actual_version,
                });
            }
        }
    }

    // Find asset for current platform
    let asset_name = platform::asset_name(spec.name);
    let asset = github::find_asset(&release, &asset_name).ok_or_else(|| {
        UpdateError::Github(IiiGithubError::Network(
            super::error::NetworkError::AssetNotFound {
                binary: spec.name.to_string(),
                platform: platform::current_target().to_string(),
            },
        ))
    })?;

    // Find checksum asset in release (separate asset, not appended URL)
    let checksum_url = if spec.has_checksum {
        let checksum_name = platform::checksum_asset_name(spec.name);
        github::find_asset(&release, &checksum_name).map(|a| a.browser_download_url.clone())
    } else {
        None
    };

    // Capture previous version before record_install overwrites it.
    // Only consider state if the binary actually exists on disk —
    // stale state entries for missing binaries should show as fresh installs.
    let previous_version = if binary_installed {
        state.installed_version(spec.name).cloned()
    } else {
        None
    };

    if binary_installed {
        eprintln!("  Updating {} to v{}...", spec.name, latest_version);
    } else {
        eprintln!("  Installing {} v{}...", spec.name, latest_version);
    }

    let from_version_str = previous_version
        .as_ref()
        .map(|v| v.to_string())
        .unwrap_or_else(|| "unknown".to_string());

    telemetry::send_cli_update_started(spec.name, &from_version_str);

    // Download and install
    let target_path = platform::binary_path(spec.name);
    match download::download_and_install(client, spec, asset, checksum_url.as_deref(), &target_path)
        .await
    {
        Ok(()) => {
            state.record_install(spec.name, latest_version.clone(), asset_name);
            telemetry::send_cli_update_succeeded(
                spec.name,
                &from_version_str,
                &latest_version.to_string(),
            );
            Ok(UpdateResult::Updated {
                binary: spec.name.to_string(),
                from: previous_version,
                to: latest_version,
            })
        }
        Err(e) => {
            telemetry::send_cli_update_failed(spec.name, &from_version_str, &e.to_string());
            Err(UpdateError::Download(e))
        }
    }
}

/// Update iii itself to the latest version.
///
/// Uses the same logic as `update_binary`: only skip the download if the state
/// file explicitly records a version >= the latest release AND the binary exists
/// on disk.  When there is no state entry (e.g. first run after install.sh, or
/// migrating from the old iii-cli), the update always proceeds so the on-disk
/// binary is replaced with the latest release.
pub async fn self_update(
    client: &reqwest::Client,
    state: &mut AppState,
) -> Result<UpdateResult, UpdateError> {
    let spec = &registry::SELF_SPEC;

    platform::check_platform_support(spec)?;

    let binary_installed = is_binary_installed(spec.name);

    eprintln!("  Checking for updates to {}...", spec.name);

    let release = github::fetch_latest_release(client, spec).await?;
    let latest_version = github::parse_release_version(&release.tag_name)
        .map_err(|e| UpdateError::VersionParse(e.to_string()))?;

    // Detect the actual version of the on-disk binary rather than trusting
    // the state file, which can be stale (e.g. binary was reinstalled via
    // install.sh without updating state, or state was migrated from iii-cli).
    if binary_installed {
        if let Some(actual_version) = detect_binary_version(spec.name) {
            if actual_version >= latest_version {
                // Update state to match reality so future checks are fast
                state.record_install(
                    spec.name,
                    actual_version.clone(),
                    platform::asset_name(spec.name),
                );
                return Ok(UpdateResult::AlreadyUpToDate {
                    binary: spec.name.to_string(),
                    version: actual_version,
                });
            }
        }
    }

    let asset_name = platform::asset_name(spec.name);
    let asset = github::find_asset(&release, &asset_name).ok_or_else(|| {
        UpdateError::Github(IiiGithubError::Network(
            super::error::NetworkError::AssetNotFound {
                binary: spec.name.to_string(),
                platform: platform::current_target().to_string(),
            },
        ))
    })?;

    let checksum_url = if spec.has_checksum {
        let checksum_name = platform::checksum_asset_name(spec.name);
        github::find_asset(&release, &checksum_name).map(|a| a.browser_download_url.clone())
    } else {
        None
    };

    // Capture previous version for telemetry and result reporting.
    let previous_version = if binary_installed {
        state.installed_version(spec.name).cloned()
    } else {
        None
    };

    if binary_installed {
        eprintln!("  Updating {} to v{}...", spec.name, latest_version);
    } else {
        eprintln!("  Installing {} v{}...", spec.name, latest_version);
    }

    let from_version_str = previous_version
        .as_ref()
        .map(|v| v.to_string())
        .unwrap_or_else(|| "unknown".to_string());

    telemetry::send_cli_update_started(spec.name, &from_version_str);

    // Install to the standard managed location (~/.local/bin/iii),
    // consistent with install.sh and other managed binaries.
    let target_path = platform::binary_path(spec.name);

    match download::download_and_install(client, spec, asset, checksum_url.as_deref(), &target_path)
        .await
    {
        Ok(()) => {
            state.record_install(spec.name, latest_version.clone(), asset_name);
            telemetry::send_cli_update_succeeded(
                spec.name,
                &from_version_str,
                &latest_version.to_string(),
            );
            Ok(UpdateResult::Updated {
                binary: spec.name.to_string(),
                from: previous_version,
                to: latest_version,
            })
        }
        Err(e) => {
            telemetry::send_cli_update_failed(spec.name, &from_version_str, &e.to_string());
            Err(UpdateError::Download(e))
        }
    }
}

/// Update all installed binaries (including iii itself).
pub async fn update_all(
    client: &reqwest::Client,
    state: &mut AppState,
) -> Vec<Result<UpdateResult, UpdateError>> {
    // Self-update first
    let mut results = vec![self_update(client, state).await];

    for spec in registry::all_binaries() {
        results.push(update_binary(client, spec, state).await);
    }
    results
}

/// Result of an update operation.
#[derive(Debug)]
pub enum UpdateResult {
    Updated {
        binary: String,
        from: Option<Version>,
        to: Version,
    },
    AlreadyUpToDate {
        binary: String,
        version: Version,
    },
}

/// Errors during update.
#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error(transparent)]
    Registry(#[from] RegistryError),

    #[error(transparent)]
    Github(#[from] IiiGithubError),

    #[error("Failed to parse version: {0}")]
    VersionParse(String),

    #[error(transparent)]
    Download(#[from] download::DownloadAndInstallError),
}

/// Print the result of an update operation.
pub fn print_update_result(result: &Result<UpdateResult, UpdateError>) {
    match result {
        Ok(UpdateResult::Updated { binary, from, to }) => {
            if let Some(from) = from {
                eprintln!(
                    "  {} {} updated: {} → {}",
                    "✓".green(),
                    binary,
                    from.to_string().dimmed(),
                    to.to_string().green(),
                );
            } else {
                eprintln!(
                    "  {} {} installed: v{}",
                    "✓".green(),
                    binary,
                    to.to_string().green(),
                );
            }
        }
        Ok(UpdateResult::AlreadyUpToDate { binary, version }) => {
            eprintln!(
                "  {} {} is already up to date (v{})",
                "✓".green(),
                binary,
                version,
            );
        }
        Err(e) => {
            eprintln!("  {} {}", "error:".red(), e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    // ── run_version_check tests ─────────────────────────────────────

    #[test]
    fn run_version_check_parses_bare_version() {
        // Create a script that outputs just a version number
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("fake-binary");
        {
            let mut f = std::fs::File::create(&script).unwrap();
            writeln!(f, "#!/bin/sh\necho '1.2.3'").unwrap();
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let v = run_version_check(&script).unwrap();
        assert_eq!(v, Version::new(1, 2, 3));
    }

    #[test]
    fn run_version_check_parses_prefixed_version() {
        // "iii 0.9.0" → should extract "0.9.0"
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("fake-binary");
        {
            let mut f = std::fs::File::create(&script).unwrap();
            writeln!(f, "#!/bin/sh\necho 'iii 0.9.0'").unwrap();
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let v = run_version_check(&script).unwrap();
        assert_eq!(v, Version::new(0, 9, 0));
    }

    #[test]
    fn run_version_check_returns_none_for_nonexistent_binary() {
        let path = std::path::Path::new("/tmp/nonexistent-binary-xyz");
        assert!(run_version_check(path).is_none());
    }

    #[test]
    fn run_version_check_returns_none_for_invalid_output() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("fake-binary");
        {
            let mut f = std::fs::File::create(&script).unwrap();
            writeln!(f, "#!/bin/sh\necho 'not-a-version'").unwrap();
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        assert!(run_version_check(&script).is_none());
    }

    #[test]
    fn run_version_check_handles_trailing_whitespace() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("fake-binary");
        {
            let mut f = std::fs::File::create(&script).unwrap();
            writeln!(f, "#!/bin/sh\nprintf '2.0.0\\n  '").unwrap();
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let v = run_version_check(&script).unwrap();
        assert_eq!(v, Version::new(2, 0, 0));
    }

    // ── detect_binary_version tests ─────────────────────────────────

    #[test]
    fn detect_binary_version_returns_none_for_unknown_binary() {
        // A binary name that doesn't exist anywhere
        assert!(detect_binary_version("nonexistent-binary-zzz-12345").is_none());
    }

    // ── cli_command_for_binary tests ────────────────────────────────

    #[test]
    fn cli_command_for_binary_resolves_console() {
        assert_eq!(cli_command_for_binary("iii-console"), Some("console"));
    }

    #[test]
    fn cli_command_for_binary_resolves_tools() {
        assert_eq!(cli_command_for_binary("iii-tools"), Some("create"));
    }

    #[test]
    fn cli_command_for_binary_returns_none_for_unknown() {
        assert!(cli_command_for_binary("unknown-binary").is_none());
    }

    // ── print_update_notifications tests ────────────────────────────

    #[test]
    fn print_update_notifications_empty_does_not_panic() {
        print_update_notifications(&[]);
    }

    #[test]
    fn print_update_notifications_with_updates_does_not_panic() {
        let updates = vec![UpdateInfo {
            binary_name: "iii-console".to_string(),
            current_version: Version::new(0, 8, 0),
            latest_version: Version::new(0, 9, 0),
        }];
        print_update_notifications(&updates);
    }

    // ── print_update_result tests ───────────────────────────────────

    #[test]
    fn print_update_result_updated_with_from() {
        let result = Ok(UpdateResult::Updated {
            binary: "iii".to_string(),
            from: Some(Version::new(0, 8, 0)),
            to: Version::new(0, 9, 0),
        });
        print_update_result(&result); // should not panic
    }

    #[test]
    fn print_update_result_updated_without_from() {
        let result = Ok(UpdateResult::Updated {
            binary: "iii".to_string(),
            from: None,
            to: Version::new(0, 9, 0),
        });
        print_update_result(&result);
    }

    #[test]
    fn print_update_result_already_up_to_date() {
        let result = Ok(UpdateResult::AlreadyUpToDate {
            binary: "iii".to_string(),
            version: Version::new(0, 9, 0),
        });
        print_update_result(&result);
    }

    #[test]
    fn print_update_result_error() {
        let result: Result<UpdateResult, UpdateError> =
            Err(UpdateError::VersionParse("bad version".to_string()));
        print_update_result(&result);
    }

    // ── is_binary_installed tests ───────────────────────────────────

    #[test]
    fn is_binary_installed_false_for_nonexistent() {
        assert!(!is_binary_installed("nonexistent-binary-zzz-12345"));
    }
}