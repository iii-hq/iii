// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
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
