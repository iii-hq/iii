// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use colored::Colorize;
use semver::{Version, VersionReq};
use serde::Deserialize;

use super::state::AppState;

/// URL where advisories are hosted.
const ADVISORIES_URL: &str =
    "https://raw.githubusercontent.com/iii-hq/iii/main/cli/advisories.json";

/// The top-level advisories document.
#[derive(Debug, Deserialize)]
pub struct AdvisoriesDocument {
    #[serde(default)]
    pub advisories: Vec<Advisory>,
}

/// A single security/critical advisory.
#[derive(Debug, Deserialize)]
pub struct Advisory {
    /// Advisory identifier (e.g., "ADV-2026-001")
    pub id: String,
    /// Severity level: "critical", "high", "medium", "low"
    pub severity: String,
    /// The binary affected (e.g., "iii-console")
    pub affected_binary: String,
    /// Semver range of affected versions (e.g., "<0.2.5")
    pub affected_versions: String,
    /// The version that fixes the issue
    pub fixed_version: String,
    /// Human-readable message
    pub message: String,
    /// URL with more details (optional)
    #[serde(default)]
    pub url: Option<String>,
}

/// An advisory that matches an installed binary.
#[derive(Debug)]
pub struct MatchedAdvisory {
    pub advisory: Advisory,
    pub installed_version: Version,
}

/// Fetch advisories from the remote URL.
pub async fn fetch_advisories(
    client: &reqwest::Client,
) -> Result<AdvisoriesDocument, reqwest::Error> {
    let response = client.get(ADVISORIES_URL).send().await?;

    if !response.status().is_success() {
        // Return empty advisories on non-200 responses
        return Ok(AdvisoriesDocument {
            advisories: Vec::new(),
        });
    }

    let doc: AdvisoriesDocument = response.json().await.unwrap_or(AdvisoriesDocument {
        advisories: Vec::new(),
    });

    Ok(doc)
}

/// Check advisories against installed binaries.
pub fn check_advisories(advisories: &AdvisoriesDocument, state: &AppState) -> Vec<MatchedAdvisory> {
    let mut matched = Vec::new();

    for advisory in &advisories.advisories {
        if let Some(binary_state) = state.binaries.get(&advisory.affected_binary) {
            // Parse the affected version range
            if let Ok(req) = VersionReq::parse(&advisory.affected_versions) {
                if req.matches(&binary_state.version) {
                    matched.push(MatchedAdvisory {
                        advisory: Advisory {
                            id: advisory.id.clone(),
                            severity: advisory.severity.clone(),
                            affected_binary: advisory.affected_binary.clone(),
                            affected_versions: advisory.affected_versions.clone(),
                            fixed_version: advisory.fixed_version.clone(),
                            message: advisory.message.clone(),
                            url: advisory.url.clone(),
                        },
                        installed_version: binary_state.version.clone(),
                    });
                }
            }
        }
    }

    matched
}

/// Print advisory warnings to stderr.
/// Critical advisories use red/bold, others use yellow.
pub fn print_advisory_warnings(matched: &[MatchedAdvisory]) {
    if matched.is_empty() {
        return;
    }

    eprintln!();
    for m in matched {
        let prefix = match m.advisory.severity.as_str() {
            "critical" => "CRITICAL".red().bold().to_string(),
            "high" => "WARNING".red().to_string(),
            _ => "NOTICE".yellow().to_string(),
        };

        eprintln!(
            "  {} [{}] {} (installed: v{}, fixed in: v{})",
            prefix,
            m.advisory.id,
            m.advisory.message,
            m.installed_version,
            m.advisory.fixed_version,
        );

        // Show CLI command to update
        let cmd = super::registry::REGISTRY
            .iter()
            .find(|s| s.name == m.advisory.affected_binary)
            .and_then(|s| s.commands.first())
            .map(|c| c.cli_command)
            .unwrap_or(&m.advisory.affected_binary);

        eprintln!("         Run: {}", format!("iii update {}", cmd).bold());

        if let Some(url) = &m.advisory.url {
            eprintln!("         Details: {}", url);
        }
    }
    eprintln!();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::state::BinaryState;
    use chrono::Utc;
    use std::collections::HashMap;

    fn make_state(binary: &str, version: &str) -> AppState {
        let mut binaries = HashMap::new();
        binaries.insert(
            binary.to_string(),
            BinaryState {
                version: Version::parse(version).unwrap(),
                installed_at: Utc::now(),
                asset_name: "test.tar.gz".to_string(),
            },
        );
        AppState {
            binaries,
            last_update_check: None,
            update_check_interval_hours: 24,
        }
    }

    #[test]
    fn test_matching_advisory() {
        let doc = AdvisoriesDocument {
            advisories: vec![Advisory {
                id: "ADV-2026-001".to_string(),
                severity: "critical".to_string(),
                affected_binary: "iii-console".to_string(),
                affected_versions: "<0.2.5".to_string(),
                fixed_version: "0.2.5".to_string(),
                message: "Security vulnerability".to_string(),
                url: Some("https://example.com".to_string()),
            }],
        };

        let state = make_state("iii-console", "0.2.4");
        let matched = check_advisories(&doc, &state);
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].advisory.id, "ADV-2026-001");
    }

    #[test]
    fn test_non_matching_advisory() {
        let doc = AdvisoriesDocument {
            advisories: vec![Advisory {
                id: "ADV-2026-001".to_string(),
                severity: "critical".to_string(),
                affected_binary: "iii-console".to_string(),
                affected_versions: "<0.2.5".to_string(),
                fixed_version: "0.2.5".to_string(),
                message: "Security vulnerability".to_string(),
                url: None,
            }],
        };

        let state = make_state("iii-console", "0.2.5");
        let matched = check_advisories(&doc, &state);
        assert_eq!(matched.len(), 0);
    }

    #[test]
    fn test_advisory_for_uninstalled_binary() {
        let doc = AdvisoriesDocument {
            advisories: vec![Advisory {
                id: "ADV-2026-001".to_string(),
                severity: "critical".to_string(),
                affected_binary: "iii-console".to_string(),
                affected_versions: "<0.2.5".to_string(),
                fixed_version: "0.2.5".to_string(),
                message: "Security vulnerability".to_string(),
                url: None,
            }],
        };

        let state = AppState::default();
        let matched = check_advisories(&doc, &state);
        assert_eq!(matched.len(), 0);
    }
}
