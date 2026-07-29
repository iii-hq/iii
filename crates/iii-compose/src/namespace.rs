// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Project namespace derivation.
//!
//! Every child of one compose project registers in the same engine namespace,
//! which is what lets two projects declare a `state` worker each without
//! colliding. An explicit `--namespace` wins; otherwise the namespace is
//! derived from the project name plus a digest of the canonical compose path,
//! so the same file always yields the same namespace and two copies of the same
//! project in different directories yield different ones.

use std::path::Path;

use sha2::{Digest, Sha256};

/// Digest length in hex characters. Eight is enough to keep accidental
/// collisions negligible while leaving the namespace readable in logs.
const DIGEST_CHARS: usize = 8;

pub fn project_namespace(
    explicit: Option<&str>,
    project_name: &str,
    canonical_compose_path: &Path,
) -> String {
    if let Some(explicit) = explicit.map(str::trim).filter(|value| !value.is_empty()) {
        return explicit.to_string();
    }
    let digest = hex::encode(Sha256::digest(
        canonical_compose_path.to_string_lossy().as_bytes(),
    ));
    format!("{}-{}", sanitize(project_name), &digest[..DIGEST_CHARS])
}

/// Namespaces travel into queue names, metric labels and log lines, so the
/// derived form stays inside `[a-z0-9_-]`.
fn sanitize(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut last_was_dash = false;
    for ch in value.chars() {
        let mapped = match ch {
            'a'..='z' | '0'..='9' | '_' | '-' => ch,
            'A'..='Z' => ch.to_ascii_lowercase(),
            _ => '-',
        };
        if mapped == '-' {
            if last_was_dash {
                continue;
            }
            last_was_dash = true;
        } else {
            last_was_dash = false;
        }
        out.push(mapped);
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "project".to_string()
    } else {
        trimmed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn explicit_namespace_wins() {
        let ns = project_namespace(Some("orders"), "ignored", &PathBuf::from("/srv/a.yaml"));
        assert_eq!(ns, "orders");
    }

    #[test]
    fn blank_explicit_namespace_falls_back_to_derivation() {
        let ns = project_namespace(Some("   "), "orders", &PathBuf::from("/srv/a.yaml"));
        assert!(ns.starts_with("orders-"), "unexpected namespace: {ns}");
    }

    #[test]
    fn derivation_is_stable_and_path_scoped() {
        let a = project_namespace(None, "orders", &PathBuf::from("/srv/a/worker-compose.yaml"));
        let again = project_namespace(None, "orders", &PathBuf::from("/srv/a/worker-compose.yaml"));
        let b = project_namespace(None, "orders", &PathBuf::from("/srv/b/worker-compose.yaml"));

        assert_eq!(a, again);
        assert_ne!(a, b);
        assert_eq!(a.len(), "orders-".len() + DIGEST_CHARS);
    }

    #[test]
    fn project_names_are_sanitized() {
        let ns = project_namespace(None, "Orders API!", &PathBuf::from("/srv/a.yaml"));
        assert!(ns.starts_with("orders-api-"), "unexpected namespace: {ns}");
    }
}
