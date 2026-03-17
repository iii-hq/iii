use std::time::Duration;

use colored::Colorize;
use semver::Version;

use crate::error::RegistryError;
use crate::github::{self, IiiGithubError};
use crate::registry::{self, BinarySpec};
use crate::state::AppState;
use crate::{download, platform, telemetry};

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
            "  {} Update available: {} {} → {} (run `iii-cli update {}`)",
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

    // Check if already up to date (only if the binary file actually exists on disk)
    if binary_installed {
        if let Some(installed) = state.installed_version(spec.name) {
            if *installed >= latest_version {
                return Ok(UpdateResult::AlreadyUpToDate {
                    binary: spec.name.to_string(),
                    version: installed.clone(),
                });
            }
        }
    }

    // Find asset for current platform
    let asset_name = platform::asset_name(spec.name);
    let asset = github::find_asset(&release, &asset_name).ok_or_else(|| {
        UpdateError::Github(IiiGithubError::Network(
            crate::error::NetworkError::AssetNotFound {
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
    match download::download_and_install(client, spec, asset, checksum_url.as_deref(), &target_path).await {
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

/// Update iii-cli itself to the latest version.
pub async fn self_update(
    client: &reqwest::Client,
    state: &mut AppState,
) -> Result<UpdateResult, UpdateError> {
    let spec = &registry::SELF_SPEC;

    platform::check_platform_support(spec)?;

    eprintln!("  Checking for updates to {}...", spec.name);

    let release = github::fetch_latest_release(client, spec).await?;
    let latest_version = github::parse_release_version(&release.tag_name)
        .map_err(|e| UpdateError::VersionParse(e.to_string()))?;

    // Use the installed binary version from state if available,
    // falling back to the compile-time version of the running binary.
    // This prevents re-downloading when the managed binary is already up-to-date
    // but the running binary is a dev build with an older compile-time version.
    let current_version = state
        .installed_version(spec.name)
        .cloned()
        .unwrap_or_else(|| {
            Version::parse(env!("CARGO_PKG_VERSION"))
                .expect("CARGO_PKG_VERSION is always valid semver")
        });

    if current_version >= latest_version {
        return Ok(UpdateResult::AlreadyUpToDate {
            binary: spec.name.to_string(),
            version: current_version,
        });
    }

    let asset_name = platform::asset_name(spec.name);
    let asset = github::find_asset(&release, &asset_name).ok_or_else(|| {
        UpdateError::Github(IiiGithubError::Network(
            crate::error::NetworkError::AssetNotFound {
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

    eprintln!("  Updating {} to v{}...", spec.name, latest_version);

    let from_version_str = current_version.to_string();

    telemetry::send_cli_update_started(spec.name, &from_version_str);

    // Install to the standard managed location (~/.local/bin/iii-cli),
    // consistent with install.sh and other managed binaries.
    let target_path = platform::binary_path(spec.name);

    match download::download_and_install(client, spec, asset, checksum_url.as_deref(), &target_path).await {
        Ok(()) => {
            state.record_install(spec.name, latest_version.clone(), asset_name);
            telemetry::send_cli_update_succeeded(
                spec.name,
                &from_version_str,
                &latest_version.to_string(),
            );
            Ok(UpdateResult::Updated {
                binary: spec.name.to_string(),
                from: Some(current_version),
                to: latest_version,
            })
        }
        Err(e) => {
            telemetry::send_cli_update_failed(spec.name, &from_version_str, &e.to_string());
            Err(UpdateError::Download(e))
        }
    }
}

/// Update all installed binaries (including iii-cli itself).
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
