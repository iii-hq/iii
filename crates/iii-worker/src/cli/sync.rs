// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! `iii worker sync` — lock-as-truth install path and drift detection.
//!
//! Slice A.1 (this file): the primitives — manifest hashing, drift
//! detection, and the `SyncError` rendering contract for user-facing
//! errors. The `handle_worker_sync` callsite in `managed.rs` wires the
//! `--frozen` path to these primitives. Install-from-lock and CAS land
//! in later slices.

use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

use super::lockfile::MANIFEST_HASH_PREFIX;

/// Compute the canonical hash of a declared-dependency set.
///
/// The input is the parser output from `worker_manifest_deps::parse_dependencies`
/// — a sorted `BTreeMap<String, String>` of `name → semver-range`. Serialization
/// is explicit and whitespace-stable: one line per entry, `"{name}={range}\n"`.
/// YAML quoting, comments, and key ordering in the source file are normalized
/// away by the parser before they reach this function, so a `yamlfmt` round-trip
/// produces the same hash.
///
/// Output is prefixed with `MANIFEST_HASH_PREFIX` (`"sha256:v1:"`). The `v1`
/// segment is the ALGORITHM version — if we ever change what bytes we hash or
/// how we serialize them, bump to `v2:` so existing locks are recognized as
/// stale and re-hashed rather than silently mismatching.
pub fn compute_manifest_hash(deps: &BTreeMap<String, String>) -> String {
    let mut hasher = Sha256::new();
    for (name, range) in deps {
        hasher.update(name.as_bytes());
        hasher.update(b"=");
        hasher.update(range.as_bytes());
        hasher.update(b"\n");
    }
    let digest = hasher.finalize();
    format!("{MANIFEST_HASH_PREFIX}{}", hex::encode(digest))
}

/// What changed between the manifest's declared deps and the locked
/// transitive graph's root declared deps. `changed` holds the name plus
/// `(old_range, new_range)`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DriftReport {
    pub added: Vec<(String, String)>,
    pub removed: Vec<(String, String)>,
    pub changed: Vec<(String, String, String)>,
}

impl DriftReport {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.changed.is_empty()
    }
}

/// Compare the current manifest-declared deps against the deps recorded
/// in the lockfile. Returns `None` when they agree. Returns `Some(report)`
/// naming every added, removed, and range-changed entry, sorted by name
/// within each bucket so diffs are stable in CI output.
pub fn detect_drift(
    manifest_deps: &BTreeMap<String, String>,
    lock_deps: &BTreeMap<String, String>,
) -> Option<DriftReport> {
    let mut report = DriftReport::default();

    for (name, range) in manifest_deps {
        match lock_deps.get(name) {
            None => report.added.push((name.clone(), range.clone())),
            Some(locked) if locked != range => {
                report
                    .changed
                    .push((name.clone(), locked.clone(), range.clone()));
            }
            Some(_) => {}
        }
    }
    for (name, range) in lock_deps {
        if !manifest_deps.contains_key(name) {
            report.removed.push((name.clone(), range.clone()));
        }
    }

    // Inputs are BTreeMaps so iteration is already sorted; the Vec order
    // inherits that. Explicit here so reorderings above stay safe.
    report.added.sort();
    report.removed.sort();
    report.changed.sort();

    if report.is_empty() {
        None
    } else {
        Some(report)
    }
}

/// User-facing errors emitted by the sync command. Each variant carries
/// enough context to render the `error: / fix: / docs:` three-line format
/// committed to in the plan (D8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncError {
    /// The manifest and lockfile disagree. Rendered as a structured diff.
    Drift { report: DriftReport },
    /// Lockfile on disk but malformed or unreadable.
    CorruptLock { path: String, reason: String },
    /// `iii.worker.yaml` does not exist but the lock recorded a
    /// manifest_hash. Distinct from drift: the manifest isn't *changed*,
    /// it's *gone* — the user needs to restore the file or remove the
    /// lock, not run `--refresh`.
    ManifestMissing { lock_deps: BTreeMap<String, String> },
    /// `iii.worker.yaml` exists but failed to parse. The reason carries
    /// the parser's message so the user can fix the YAML directly.
    CorruptManifest { reason: String },
    /// The lock's `manifest_hash` does not match
    /// `compute_manifest_hash(&declared_dependencies)` AND
    /// declared_dependencies equals the manifest deps. The lock is
    /// internally inconsistent (likely tampered or partially edited);
    /// drift attribution would be empty so we surface the corruption
    /// directly instead.
    LockInconsistent,
    // CacheMiss and NetworkRequired variants land in Slice A.3 (install-
    // from-lock) and Lane B (CAS). Not included here to avoid dead code.
}

impl SyncError {
    /// Three-line rendering: `error:`, per-item diff, `fix:`, `docs:`.
    /// Stable byte-for-byte output so downstream snapshot tests can pin
    /// exact bytes. Colors are intentionally omitted here — the callsite
    /// in `managed.rs` wraps with `colored` when writing to a TTY, but
    /// golden tests compare the raw template.
    pub fn render(&self) -> String {
        match self {
            Self::Drift { report } => render_drift(report),
            Self::CorruptLock { path, reason } => {
                format!(
                    "error: iii.lock at {path} is unreadable: {reason}\n\
                     fix: restore the file from git or run `iii worker sync --refresh`\n\
                     docs: https://iii.dev/docs/sync#corrupt-lock\n"
                )
            }
            Self::ManifestMissing { lock_deps } => render_manifest_missing(lock_deps),
            Self::CorruptManifest { reason } => {
                format!(
                    "error: iii.worker.yaml failed to parse: {reason}\n\
                     fix: correct the YAML in iii.worker.yaml and re-run\n\
                     docs: https://iii.dev/docs/sync#corrupt-manifest\n"
                )
            }
            Self::LockInconsistent => String::from(
                "error: iii.lock is internally inconsistent: \
                 manifest_hash does not match declared_dependencies\n\
                 fix: restore iii.lock from git, or regenerate with `iii worker add`\n\
                 docs: https://iii.dev/docs/sync#corrupt-lock\n",
            ),
        }
    }
}

fn render_manifest_missing(lock_deps: &BTreeMap<String, String>) -> String {
    let mut out = String::from("error: iii.worker.yaml is missing but iii.lock expects it\n");
    for (name, range) in lock_deps {
        out.push_str(&format!("  - {name} \"{range}\" (in lock)\n"));
    }
    out.push_str("fix: restore iii.worker.yaml from git, or remove iii.lock\n");
    out.push_str("docs: https://iii.dev/docs/sync#missing-manifest\n");
    out
}

fn render_drift(report: &DriftReport) -> String {
    let mut out = String::from("error: iii.worker.yaml drift vs iii.lock\n");
    for (name, range) in &report.added {
        out.push_str(&format!("  + {name} \"{range}\" (not in lock)\n"));
    }
    for (name, range) in &report.removed {
        out.push_str(&format!(
            "  - {name} \"{range}\" (in lock, not in manifest)\n"
        ));
    }
    for (name, old, new) in &report.changed {
        out.push_str(&format!("  ~ {name} \"{old}\" → \"{new}\"\n"));
    }
    out.push_str("fix: run `iii worker sync --refresh && git add iii.lock`\n");
    out.push_str("docs: https://iii.dev/docs/sync#drift\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deps(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[test]
    fn compute_manifest_hash_is_deterministic() {
        let a = deps(&[("alpha", "^1.0"), ("beta", "~2.0")]);
        let b = deps(&[("alpha", "^1.0"), ("beta", "~2.0")]);
        assert_eq!(compute_manifest_hash(&a), compute_manifest_hash(&b));
    }

    #[test]
    fn compute_manifest_hash_is_key_order_independent() {
        // BTreeMap sorts on insert, so this is really checking that iteration
        // order in the hasher matches the sort order. Documented here so a
        // future switch to HashMap doesn't silently break.
        let mut forward = BTreeMap::new();
        forward.insert("alpha".to_string(), "^1.0".to_string());
        forward.insert("beta".to_string(), "~2.0".to_string());

        let mut reverse = BTreeMap::new();
        reverse.insert("beta".to_string(), "~2.0".to_string());
        reverse.insert("alpha".to_string(), "^1.0".to_string());

        assert_eq!(
            compute_manifest_hash(&forward),
            compute_manifest_hash(&reverse)
        );
    }

    #[test]
    fn compute_manifest_hash_distinguishes_range_changes() {
        let a = deps(&[("alpha", "^1.0")]);
        let b = deps(&[("alpha", "^1.1")]);
        assert_ne!(compute_manifest_hash(&a), compute_manifest_hash(&b));
    }

    #[test]
    fn compute_manifest_hash_distinguishes_dep_set_changes() {
        let a = deps(&[("alpha", "^1.0")]);
        let b = deps(&[("alpha", "^1.0"), ("beta", "^2.0")]);
        assert_ne!(compute_manifest_hash(&a), compute_manifest_hash(&b));
    }

    #[test]
    fn compute_manifest_hash_empty_is_stable() {
        // An empty deps map still produces a deterministic well-formed hash.
        // This is the hash embedded into locks created by projects with no
        // declared dependencies — it must be distinguishable from "no hash
        // was ever computed" (None in the lockfile field).
        let h = compute_manifest_hash(&BTreeMap::new());
        assert!(h.starts_with(MANIFEST_HASH_PREFIX));
        // SHA-256 of the empty string.
        let expected_hex = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        assert_eq!(h, format!("{MANIFEST_HASH_PREFIX}{expected_hex}"));
    }

    #[test]
    fn compute_manifest_hash_has_algorithm_version_prefix() {
        // The "v1:" segment is load-bearing — it lets future iii detect
        // hash-algo bumps. If someone drops it, this test fails.
        let h = compute_manifest_hash(&deps(&[("alpha", "^1.0")]));
        assert!(h.starts_with("sha256:v1:"), "got: {h}");
    }

    #[test]
    fn detect_drift_none_on_exact_match() {
        let d = deps(&[("alpha", "^1.0"), ("beta", "~2.0")]);
        assert_eq!(detect_drift(&d, &d), None);
    }

    #[test]
    fn detect_drift_none_on_both_empty() {
        assert_eq!(detect_drift(&BTreeMap::new(), &BTreeMap::new()), None);
    }

    #[test]
    fn detect_drift_flags_added_deps() {
        let manifest = deps(&[("alpha", "^1.0"), ("beta", "~2.0")]);
        let lock = deps(&[("alpha", "^1.0")]);
        let report = detect_drift(&manifest, &lock).unwrap();
        assert_eq!(report.added, vec![("beta".to_string(), "~2.0".to_string())]);
        assert!(report.removed.is_empty());
        assert!(report.changed.is_empty());
    }

    #[test]
    fn detect_drift_flags_removed_deps() {
        let manifest = deps(&[("alpha", "^1.0")]);
        let lock = deps(&[("alpha", "^1.0"), ("gone", "^9.0")]);
        let report = detect_drift(&manifest, &lock).unwrap();
        assert_eq!(
            report.removed,
            vec![("gone".to_string(), "^9.0".to_string())]
        );
    }

    #[test]
    fn detect_drift_flags_range_changes() {
        let manifest = deps(&[("alpha", "^1.1")]);
        let lock = deps(&[("alpha", "^1.0")]);
        let report = detect_drift(&manifest, &lock).unwrap();
        assert_eq!(
            report.changed,
            vec![("alpha".to_string(), "^1.0".to_string(), "^1.1".to_string())]
        );
    }

    #[test]
    fn detect_drift_buckets_mixed_changes() {
        let manifest = deps(&[("added", "^1.0"), ("changed", "^2.1"), ("same", "^3.0")]);
        let lock = deps(&[("changed", "^2.0"), ("removed", "^9.0"), ("same", "^3.0")]);
        let report = detect_drift(&manifest, &lock).unwrap();
        assert_eq!(
            report.added,
            vec![("added".to_string(), "^1.0".to_string())]
        );
        assert_eq!(
            report.removed,
            vec![("removed".to_string(), "^9.0".to_string())]
        );
        assert_eq!(
            report.changed,
            vec![(
                "changed".to_string(),
                "^2.0".to_string(),
                "^2.1".to_string()
            )]
        );
    }

    #[test]
    fn render_drift_added_only_golden() {
        let err = SyncError::Drift {
            report: DriftReport {
                added: vec![("helper-worker".into(), "^2.0.0".into())],
                ..Default::default()
            },
        };
        let expected = "\
error: iii.worker.yaml drift vs iii.lock
  + helper-worker \"^2.0.0\" (not in lock)
fix: run `iii worker sync --refresh && git add iii.lock`
docs: https://iii.dev/docs/sync#drift
";
        assert_eq!(err.render(), expected);
    }

    #[test]
    fn render_drift_removed_only_golden() {
        let err = SyncError::Drift {
            report: DriftReport {
                removed: vec![("stale-worker".into(), "^1.0.0".into())],
                ..Default::default()
            },
        };
        let expected = "\
error: iii.worker.yaml drift vs iii.lock
  - stale-worker \"^1.0.0\" (in lock, not in manifest)
fix: run `iii worker sync --refresh && git add iii.lock`
docs: https://iii.dev/docs/sync#drift
";
        assert_eq!(err.render(), expected);
    }

    #[test]
    fn render_drift_range_change_golden() {
        let err = SyncError::Drift {
            report: DriftReport {
                changed: vec![("http-worker".into(), "^1.0.0".into(), "^1.1.0".into())],
                ..Default::default()
            },
        };
        let expected = "\
error: iii.worker.yaml drift vs iii.lock
  ~ http-worker \"^1.0.0\" → \"^1.1.0\"
fix: run `iii worker sync --refresh && git add iii.lock`
docs: https://iii.dev/docs/sync#drift
";
        assert_eq!(err.render(), expected);
    }

    #[test]
    fn render_drift_mixed_golden() {
        // The exact shape committed to in the plan's D8 example.
        let err = SyncError::Drift {
            report: DriftReport {
                added: vec![("helper-worker".into(), "^2.0.0".into())],
                changed: vec![("http-worker".into(), "^1.0.0".into(), "^1.1.0".into())],
                ..Default::default()
            },
        };
        let expected = "\
error: iii.worker.yaml drift vs iii.lock
  + helper-worker \"^2.0.0\" (not in lock)
  ~ http-worker \"^1.0.0\" → \"^1.1.0\"
fix: run `iii worker sync --refresh && git add iii.lock`
docs: https://iii.dev/docs/sync#drift
";
        assert_eq!(err.render(), expected);
    }

    #[test]
    fn render_corrupt_lock() {
        let err = SyncError::CorruptLock {
            path: "iii.lock".into(),
            reason: "unexpected character at line 3".into(),
        };
        let expected = "\
error: iii.lock at iii.lock is unreadable: unexpected character at line 3
fix: restore the file from git or run `iii worker sync --refresh`
docs: https://iii.dev/docs/sync#corrupt-lock
";
        assert_eq!(err.render(), expected);
    }

    #[test]
    fn render_manifest_missing_golden() {
        let err = SyncError::ManifestMissing {
            lock_deps: deps(&[("alpha", "^1.0.0"), ("beta", "^2.0.0")]),
        };
        let expected = "\
error: iii.worker.yaml is missing but iii.lock expects it
  - alpha \"^1.0.0\" (in lock)
  - beta \"^2.0.0\" (in lock)
fix: restore iii.worker.yaml from git, or remove iii.lock
docs: https://iii.dev/docs/sync#missing-manifest
";
        assert_eq!(err.render(), expected);
    }

    #[test]
    fn render_corrupt_manifest_golden() {
        let err = SyncError::CorruptManifest {
            reason: "`dependencies` must be a mapping".into(),
        };
        let expected = "\
error: iii.worker.yaml failed to parse: `dependencies` must be a mapping
fix: correct the YAML in iii.worker.yaml and re-run
docs: https://iii.dev/docs/sync#corrupt-manifest
";
        assert_eq!(err.render(), expected);
    }

    #[test]
    fn render_lock_inconsistent_golden() {
        let expected = "\
error: iii.lock is internally inconsistent: manifest_hash does not match declared_dependencies
fix: restore iii.lock from git, or regenerate with `iii worker add`
docs: https://iii.dev/docs/sync#corrupt-lock
";
        assert_eq!(SyncError::LockInconsistent.render(), expected);
    }
}
