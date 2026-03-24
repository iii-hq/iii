// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use thiserror::Error;

#[derive(Error, Debug)]
#[allow(dead_code)]
pub enum IiiCliError {
    #[error(transparent)]
    Network(#[from] NetworkError),

    #[error(transparent)]
    Download(#[from] DownloadError),

    #[error(transparent)]
    Extract(#[from] ExtractError),

    #[error(transparent)]
    Storage(#[from] StorageError),

    #[error(transparent)]
    Exec(#[from] ExecError),

    #[error(transparent)]
    Registry(#[from] RegistryError),

    #[error(transparent)]
    State(#[from] StateError),

    #[error(transparent)]
    Worker(#[from] WorkerError),
}

#[derive(Error, Debug)]
pub enum NetworkError {
    #[error("HTTP request failed: {0}")]
    RequestFailed(#[from] reqwest::Error),

    #[error(
        "GitHub API rate limit exceeded. Set GITHUB_TOKEN or III_GITHUB_TOKEN environment variable for higher limits."
    )]
    RateLimited,

    #[error("Release asset not found for platform {platform}: {binary}")]
    AssetNotFound { binary: String, platform: String },
}

#[derive(Error, Debug)]
pub enum DownloadError {
    #[error("Download failed: {0}")]
    Failed(String),

    #[error(
        "SHA256 checksum mismatch for {asset}. Expected: {expected}, got: {actual}. The downloaded file may be corrupted. Try running the command again."
    )]
    ChecksumMismatch {
        asset: String,
        expected: String,
        actual: String,
    },

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
}

#[derive(Error, Debug)]
pub enum ExtractError {
    #[error("Failed to extract archive: {0}")]
    ExtractionFailed(String),

    #[error("IO error during extraction: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("Failed to create storage directory {path}: {source}")]
    CreateDir {
        path: String,
        source: std::io::Error,
    },

    #[error("Failed to write file {path}: {source}")]
    #[allow(dead_code)]
    WriteFile {
        path: String,
        source: std::io::Error,
    },

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Error, Debug)]
pub enum ExecError {
    #[error("Failed to execute binary {binary}: {source}")]
    SpawnFailed {
        binary: String,
        source: std::io::Error,
    },

    #[error("Binary not found at {path}. Try running the command again to re-download.")]
    BinaryNotFound { path: String },
}

#[derive(Error, Debug)]
pub enum RegistryError {
    #[error("Unknown command: '{command}'. Run 'iii --help' to see available commands.")]
    UnknownCommand { command: String },

    #[error("{binary} is not available for {platform}. Supported platforms: {supported}")]
    UnsupportedPlatform {
        binary: String,
        platform: String,
        supported: String,
    },

    #[error("No releases found for {binary}. This binary may not yet be available for download.")]
    NoReleasesAvailable { binary: String },
}

#[derive(Error, Debug)]
pub enum StateError {
    #[error("Failed to read state file: {0}")]
    ReadFailed(String),

    #[error("Failed to parse state file: {0}")]
    ParseFailed(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Error, Debug)]
pub enum WorkerError {
    #[error(
        "Worker '{name}' not found in registry. Verify the spelling or run `iii worker info <name>` to check available workers."
    )]
    WorkerNotFound { name: String },

    #[error(
        "Failed to fetch worker registry from {url}: {reason}. Check your internet connection. If using a private registry, ensure GITHUB_TOKEN or III_GITHUB_TOKEN is set."
    )]
    RegistryFetchFailed { url: String, reason: String },

    #[error(
        "Invalid worker name '{name}': must be lowercase alphanumeric with hyphens (a-z, 0-9, -)"
    )]
    InvalidWorkerName { name: String },

    #[error("Failed to read/write iii.toml: {0}")]
    ManifestError(String),

    #[error(
        "Worker '{name}' is not available for {platform}. Supported platforms: {supported}. Check if a different architecture is available."
    )]
    UnsupportedPlatform {
        name: String,
        platform: String,
        supported: String,
    },

    #[error(
        "Download failed for worker '{name}': {reason}. Check your internet connection and try again. If the problem persists, set GITHUB_TOKEN or III_GITHUB_TOKEN for authenticated access."
    )]
    DownloadFailed { name: String, reason: String },

    #[error("Failed to read/write config.yaml: {0}")]
    ConfigError(String),

    #[error("Worker '{name}' is not installed. Run `iii worker list` to see installed workers.")]
    WorkerNotInstalled { name: String },

    #[error(
        "Version '{version}' not found for worker '{name}'. Check the available versions in the worker's GitHub releases."
    )]
    VersionNotFound { name: String, version: String },

    #[error("Release asset not found for worker '{name}': {reason}")]
    AssetNotFound { name: String, reason: String },

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_worker_not_found_has_actionable_guidance() {
        let err = WorkerError::WorkerNotFound {
            name: "foo".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("foo"), "Should contain worker name");
        assert!(
            msg.contains("iii worker info"),
            "Should suggest iii worker info command"
        );
    }

    #[test]
    fn test_registry_fetch_failed_suggests_token() {
        let err = WorkerError::RegistryFetchFailed {
            url: "https://example.com".to_string(),
            reason: "timeout".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("GITHUB_TOKEN"), "Should suggest setting token");
        assert!(
            msg.contains("internet connection"),
            "Should suggest checking connectivity"
        );
    }

    #[test]
    fn test_unsupported_platform_lists_platforms() {
        let err = WorkerError::UnsupportedPlatform {
            name: "foo".to_string(),
            platform: "x86_64-unknown-linux-gnu".to_string(),
            supported: "aarch64-apple-darwin".to_string(),
        };
        let msg = err.to_string();
        assert!(
            msg.contains("aarch64-apple-darwin"),
            "Should list supported platforms"
        );
    }

    #[test]
    fn test_download_failed_suggests_token() {
        let err = WorkerError::DownloadFailed {
            name: "foo".to_string(),
            reason: "connection reset".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("GITHUB_TOKEN"), "Should suggest setting token");
    }

    #[test]
    fn test_worker_not_installed_suggests_list() {
        let err = WorkerError::WorkerNotInstalled {
            name: "foo".to_string(),
        };
        let msg = err.to_string();
        assert!(
            msg.contains("iii worker list"),
            "Should suggest list command"
        );
    }

    #[test]
    fn test_version_not_found_has_guidance() {
        let err = WorkerError::VersionNotFound {
            name: "foo".to_string(),
            version: "9.9.9".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("9.9.9"), "Should contain requested version");
        assert!(
            msg.contains("GitHub releases"),
            "Should suggest checking releases"
        );
    }
}