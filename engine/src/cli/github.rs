// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use semver::Version;
use serde::Deserialize;

use super::error::{NetworkError, RegistryError};
use super::registry::BinarySpec;

/// A GitHub release from the /releases/latest endpoint.
#[derive(Debug, Deserialize)]
pub struct Release {
    pub tag_name: String,
    #[serde(default)]
    pub prerelease: bool,
    pub assets: Vec<ReleaseAsset>,
}

/// A single asset in a GitHub release.
#[derive(Debug, Deserialize)]
pub struct ReleaseAsset {
    pub name: String,
    pub browser_download_url: String,
    pub size: u64,
}

/// Build an HTTP client with proper configuration.
pub fn build_client() -> Result<reqwest::Client, reqwest::Error> {
    let mut builder = reqwest::Client::builder()
        .user_agent(format!("iii/{}", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(30));

    // Support optional GitHub token for higher rate limits
    if let Some(token) = github_token() {
        use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
        let mut headers = HeaderMap::new();
        if let Ok(val) = HeaderValue::from_str(&format!("token {}", token)) {
            headers.insert(AUTHORIZATION, val);
        }
        builder = builder.default_headers(headers);
    }

    builder.build()
}

/// Get the GitHub token from environment variables.
fn github_token() -> Option<String> {
    std::env::var("III_GITHUB_TOKEN")
        .or_else(|_| std::env::var("GITHUB_TOKEN"))
        .ok()
}

pub async fn fetch_latest_release(
    client: &reqwest::Client,
    spec: &BinarySpec,
) -> Result<Release, IiiGithubError> {
    match spec.tag_prefix {
        Some(prefix) => fetch_latest_release_by_prefix(client, spec, prefix).await,
        None => fetch_latest_release_simple(client, spec).await,
    }
}

async fn fetch_latest_release_simple(
    client: &reqwest::Client,
    spec: &BinarySpec,
) -> Result<Release, IiiGithubError> {
    let url = format!("https://api.github.com/repos/{}/releases/latest", spec.repo);

    let response = client.get(&url).send().await?;

    match response.status() {
        status if status.is_success() => {
            let release: Release = response.json().await?;
            Ok(release)
        }
        status if status == reqwest::StatusCode::FORBIDDEN => {
            Err(IiiGithubError::Network(NetworkError::RateLimited))
        }
        status if status == reqwest::StatusCode::NOT_FOUND => Err(IiiGithubError::Registry(
            RegistryError::NoReleasesAvailable {
                binary: spec.name.to_string(),
            },
        )),
        _status => Err(IiiGithubError::Network(NetworkError::RequestFailed(
            response.error_for_status().unwrap_err(),
        ))),
    }
}

async fn fetch_latest_release_by_prefix(
    client: &reqwest::Client,
    spec: &BinarySpec,
    prefix: &str,
) -> Result<Release, IiiGithubError> {
    let url = format!(
        "https://api.github.com/repos/{}/releases?per_page=30",
        spec.repo
    );

    let response = client.get(&url).send().await?;

    match response.status() {
        status if status.is_success() => {
            let releases: Vec<Release> = response.json().await?;
            let tag_prefix = format!("{}/v", prefix);

            releases
                .into_iter()
                .find(|r| r.tag_name.starts_with(&tag_prefix) && !r.prerelease)
                .ok_or_else(|| {
                    IiiGithubError::Registry(RegistryError::NoReleasesAvailable {
                        binary: spec.name.to_string(),
                    })
                })
        }
        status if status == reqwest::StatusCode::FORBIDDEN => {
            Err(IiiGithubError::Network(NetworkError::RateLimited))
        }
        _status => Err(IiiGithubError::Network(NetworkError::RequestFailed(
            response.error_for_status().unwrap_err(),
        ))),
    }
}

/// Helper error that can be either Network or Registry.
#[derive(Debug, thiserror::Error)]
pub enum IiiGithubError {
    #[error(transparent)]
    Network(#[from] NetworkError),
    #[error(transparent)]
    Registry(#[from] RegistryError),
    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),
}

/// Find the download URL for a specific asset in a release.
pub fn find_asset<'a>(release: &'a Release, asset_name: &str) -> Option<&'a ReleaseAsset> {
    release.assets.iter().find(|a| a.name == asset_name)
}

/// Parse a version from a release tag (strips leading 'v' if present).
pub fn parse_release_version(tag: &str) -> Result<Version, semver::Error> {
    let without_prefix = tag.rsplit_once('/').map(|(_, rest)| rest).unwrap_or(tag);
    let cleaned = without_prefix.strip_prefix('v').unwrap_or(without_prefix);
    Version::parse(cleaned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_release_version() {
        assert_eq!(
            parse_release_version("v0.2.4").unwrap(),
            Version::new(0, 2, 4)
        );
        assert_eq!(
            parse_release_version("0.2.4").unwrap(),
            Version::new(0, 2, 4)
        );
        assert_eq!(
            parse_release_version("v1.0.0").unwrap(),
            Version::new(1, 0, 0)
        );
        assert_eq!(
            parse_release_version("iii/v1.2.3").unwrap(),
            Version::new(1, 2, 3)
        );
        assert_eq!(
            parse_release_version("motia/v0.5.0").unwrap(),
            Version::new(0, 5, 0)
        );
    }

    #[test]
    fn test_find_asset() {
        let release = Release {
            tag_name: "v0.2.4".to_string(),
            prerelease: false,
            assets: vec![
                ReleaseAsset {
                    name: "iii-console-aarch64-apple-darwin.tar.gz".to_string(),
                    browser_download_url: "https://example.com/a".to_string(),
                    size: 1000,
                },
                ReleaseAsset {
                    name: "iii-console-x86_64-apple-darwin.tar.gz".to_string(),
                    browser_download_url: "https://example.com/b".to_string(),
                    size: 2000,
                },
            ],
        };

        let found = find_asset(&release, "iii-console-aarch64-apple-darwin.tar.gz");
        assert!(found.is_some());
        assert_eq!(found.unwrap().browser_download_url, "https://example.com/a");

        let not_found = find_asset(&release, "nonexistent.tar.gz");
        assert!(not_found.is_none());
    }

    #[test]
    fn test_github_token_not_set() {
        // In test environment, token is typically not set
        // This just exercises the function
        let _ = github_token();
    }
}
