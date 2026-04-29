// Adversarial tests for the drift-detection / atomic-write behavior
// from PR #1543. Each test pins a real-world failure mode the
// implementation must guard against.

use iii_worker::cli::lockfile::{
    LockedSource, LockedWorker, LockedWorkerType, MANIFEST_HASH_PREFIX, WorkerLockfile,
    is_valid_manifest_hash,
};
use iii_worker::cli::sync::{SyncError, compute_manifest_hash, detect_drift};
use std::collections::BTreeMap;

fn deps(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect()
}

fn image_worker(image: &str) -> LockedWorker {
    LockedWorker {
        version: "1.0.0".to_string(),
        worker_type: LockedWorkerType::Image,
        dependencies: BTreeMap::new(),
        source: Some(LockedSource::Image {
            image: image.to_string(),
        }),
    }
}

// ---------------------------------------------------------------------------
// 1. Hash determinism / canonicalization (acceptable behavior pinned)
// ---------------------------------------------------------------------------

/// Two ranges that are *semver-equivalent* but differ in whitespace must
/// produce different hashes. The lockfile records the literal text; a
/// future "normalize before hashing" change would need to re-evaluate
/// drift semantics.
#[test]
fn whitespace_in_range_changes_hash() {
    let tight = deps(&[("alpha", "^1.0")]);
    let loose = deps(&[("alpha", "^ 1.0")]);
    assert_ne!(compute_manifest_hash(&tight), compute_manifest_hash(&loose));
}

/// Worker names are restricted to `[a-zA-Z0-9._-]` (registry::validate_worker_name),
/// so they cannot contain `=` or `\n`. Even a malicious caller can't
/// produce a hash collision via embedded delimiters in ranges.
#[test]
fn ranges_with_embedded_eq_dont_collide() {
    let a = deps(&[("alpha", "=1.0.0"), ("beta", "=2.0.0")]);
    let b = deps(&[("alpha", "=1.0.0=2.0.0"), ("beta", "")]);
    assert_ne!(compute_manifest_hash(&a), compute_manifest_hash(&b));
}

// ---------------------------------------------------------------------------
// 2. Drift detection (correct behavior expected)
// ---------------------------------------------------------------------------

/// detect_drift on identical maps returns None.
#[test]
fn drift_none_when_equal_with_special_chars() {
    let d = deps(&[("alpha", ">=1.0.0, <2.0.0"), ("beta", "*")]);
    assert_eq!(detect_drift(&d, &d), None);
}

/// **Bug fix**: when handle_worker_sync detects a hash mismatch but
/// declared_dependencies happens to equal the manifest deps, the user
/// must see a `LockInconsistent` error (not a 3-line empty drift
/// report). The render output must explain the lock is corrupt and
/// point at the right remediation.
#[test]
fn lock_inconsistent_error_renders_specifically() {
    let err = SyncError::LockInconsistent;
    let rendered = err.render();
    assert!(rendered.starts_with("error: iii.lock"), "got: {rendered}");
    assert!(rendered.contains("internally inconsistent"));
    assert!(
        rendered.contains("fix:"),
        "must include a remediation line: {rendered}"
    );
    assert!(rendered.contains("docs:"));
    // Crucially: the rendered output must NOT look like an empty drift
    // report (the bug we're fixing).
    assert!(
        !rendered.contains("drift vs iii.lock"),
        "must not be confused with drift: {rendered}"
    );
}

/// **Bug fix**: when iii.worker.yaml is MISSING but the lock has
/// manifest_hash, the user must see a distinct `ManifestMissing` error
/// — not "drift vs iii.lock". The error must name the deps the lock
/// expects so the user knows what's at stake.
#[test]
fn manifest_missing_error_renders_specifically() {
    let lock_deps = deps(&[("alpha", "^1.0.0"), ("beta", "^2.0.0")]);
    let err = SyncError::ManifestMissing { lock_deps };
    let rendered = err.render();
    assert!(rendered.contains("iii.worker.yaml"), "got: {rendered}");
    assert!(rendered.contains("missing"), "got: {rendered}");
    assert!(rendered.contains("alpha"));
    assert!(rendered.contains("beta"));
    assert!(rendered.contains("fix:"));
    // It is NOT a drift report — the headline must not be "drift vs".
    assert!(
        !rendered.contains("drift vs iii.lock"),
        "manifest_missing must not be reported as drift: {rendered}"
    );
}

/// **Bug fix**: when iii.worker.yaml exists but is malformed, the user
/// must see a `CorruptManifest` error pointing at the YAML (with the
/// parser's reason), not a misleading "drift" report saying everything
/// was removed.
#[test]
fn corrupt_manifest_error_renders_specifically() {
    let err = SyncError::CorruptManifest {
        reason: "`dependencies` must be a mapping".into(),
    };
    let rendered = err.render();
    assert!(rendered.contains("iii.worker.yaml"));
    assert!(rendered.contains("`dependencies` must be a mapping"));
    assert!(rendered.contains("fix:"));
    assert!(
        !rendered.contains("drift vs iii.lock"),
        "malformed YAML must not be reported as drift: {rendered}"
    );
}

// ---------------------------------------------------------------------------
// 3. Atomic write (acceptable behavior pinned)
// ---------------------------------------------------------------------------

/// `write_to` REPLACES a symlinked iii.lock with a regular file. Pin
/// the behavior so a future change is intentional.
#[cfg(unix)]
#[test]
fn write_to_replaces_symlink_with_regular_file() {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("real.lock");
    let link = dir.path().join("iii.lock");

    std::fs::write(&real, "version: 1\nworkers: {}\n").unwrap();
    std::os::unix::fs::symlink(&real, &link).unwrap();

    let lock = WorkerLockfile::default();
    lock.write_to(&link).unwrap();

    let meta = std::fs::symlink_metadata(&link).unwrap();
    assert!(!meta.file_type().is_symlink(), "symlink was preserved!");
    let real_content = std::fs::read_to_string(&real).unwrap();
    assert_eq!(real_content, "version: 1\nworkers: {}\n");
}

/// Concurrent writers: last-writer-wins, no torn output, no leaked
/// temp files. PR explicitly says concurrent-writer protection is out
/// of scope; pin the documented behavior.
#[test]
fn concurrent_writers_dont_tear_but_last_writer_wins() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("iii.lock");

    let lock_a = {
        let mut l = WorkerLockfile::default();
        l.workers
            .insert("alpha".to_string(), image_worker("ghcr.io/x/a@sha256:aaa"));
        l
    };
    let lock_b = {
        let mut l = WorkerLockfile::default();
        l.workers
            .insert("beta".to_string(), image_worker("ghcr.io/x/b@sha256:bbb"));
        l
    };

    let p1 = path.clone();
    let p2 = path.clone();
    let h1 = std::thread::spawn(move || {
        for _ in 0..10 {
            lock_a.write_to(&p1).unwrap();
        }
    });
    let h2 = std::thread::spawn(move || {
        for _ in 0..10 {
            lock_b.write_to(&p2).unwrap();
        }
    });
    h1.join().unwrap();
    h2.join().unwrap();

    let parsed = WorkerLockfile::read_from(&path).unwrap();
    let names: Vec<&str> = parsed.workers.keys().map(|s| s.as_str()).collect();
    assert!(
        names == vec!["alpha"] || names == vec!["beta"],
        "got: {names:?}"
    );

    let stragglers: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n != "iii.lock")
        .collect();
    assert!(
        stragglers.is_empty(),
        "found leftover temps: {stragglers:?}"
    );
}

/// Atomic-rename contract: a concurrent reader never sees a partial
/// write. Stress test with 1000 reads against a continuously-rewriting
/// writer thread.
#[test]
fn concurrent_reader_never_sees_partial_write() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("iii.lock");

    let mut seeded = WorkerLockfile::default();
    seeded
        .workers
        .insert("seed".to_string(), image_worker("ghcr.io/x/s@sha256:ccc"));
    seeded.write_to(&path).unwrap();

    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop_w = stop.clone();
    let p_w = path.clone();

    let writer = std::thread::spawn(move || {
        let mut a = WorkerLockfile::default();
        a.workers
            .insert("alpha".to_string(), image_worker("ghcr.io/x/a@sha256:aaa"));
        let mut b = WorkerLockfile::default();
        b.workers
            .insert("beta".to_string(), image_worker("ghcr.io/x/b@sha256:bbb"));
        while !stop_w.load(std::sync::atomic::Ordering::Relaxed) {
            a.write_to(&p_w).unwrap();
            b.write_to(&p_w).unwrap();
        }
    });

    let mut parse_errors = 0usize;
    for _ in 0..1000 {
        match WorkerLockfile::read_from(&path) {
            Ok(_) => {}
            Err(e) => {
                parse_errors += 1;
                eprintln!("torn read: {e}");
            }
        }
    }

    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    writer.join().unwrap();

    assert_eq!(parse_errors, 0, "atomic-rename contract violated");
}

// ---------------------------------------------------------------------------
// 4. Lockfile validation (correct behavior expected — bugs fixed)
// ---------------------------------------------------------------------------

/// **Bug fix**: ranges in `declared_dependencies` must parse as semver.
/// The manifest parser enforces this; the lockfile parser must too, so
/// hand-edited locks can't smuggle garbage past validation.
#[test]
fn lockfile_rejects_garbage_declared_dependency_range() {
    // Hash + declared_dependencies must be paired (see fix below); the
    // hash here doesn't need to match because the semver check fires
    // first.
    let yaml = format!(
        "version: 1\nmanifest_hash: \"{prefix}{hex}\"\ndeclared_dependencies:\n  alpha: \"this-is-not-semver-at-all\"\nworkers: {{}}\n",
        prefix = MANIFEST_HASH_PREFIX,
        hex = "0".repeat(64),
    );
    let err = WorkerLockfile::from_yaml(&yaml).unwrap_err();
    assert!(err.contains("alpha"), "got: {err}");
    assert!(
        err.to_lowercase().contains("semver"),
        "error must name the bad range: {err}"
    );
}

/// **Bug fix**: a lock with `declared_dependencies` but no
/// `manifest_hash` is internally inconsistent — drift detection would
/// silently skip. Reject the combination at validation time so
/// stripping `manifest_hash:` from the lock doesn't bypass drift.
#[test]
fn lockfile_rejects_declared_deps_without_manifest_hash() {
    let yaml = "version: 1\ndeclared_dependencies:\n  alpha: \"^1.0.0\"\nworkers: {}\n";
    let err = WorkerLockfile::from_yaml(yaml).unwrap_err();
    assert!(
        err.contains("manifest_hash"),
        "error must explain the missing hash: {err}"
    );
}

/// **Bug fix**: when both `manifest_hash` and `declared_dependencies`
/// are present, the hash must equal `compute_manifest_hash(&declared)`.
/// A hand-edited lock that breaks this invariant must be rejected.
#[test]
fn lockfile_rejects_hash_mismatch_against_declared() {
    // Hash of {alpha: ^1.0.0}, but declared is {alpha: ^9.9.9} — the
    // lock would silently pass drift checks until the manifest moves.
    let real_hash = compute_manifest_hash(&deps(&[("alpha", "^1.0.0")]));
    let yaml = format!(
        "version: 1\nmanifest_hash: \"{real_hash}\"\ndeclared_dependencies:\n  alpha: \"^9.9.9\"\nworkers: {{}}\n",
    );
    let err = WorkerLockfile::from_yaml(&yaml).unwrap_err();
    assert!(
        err.contains("manifest_hash") || err.to_lowercase().contains("inconsistent"),
        "error must explain the inconsistency: {err}"
    );
}

/// Sanity: a consistent lock (hash matches declared_dependencies) is
/// accepted.
#[test]
fn lockfile_accepts_consistent_hash_and_declared() {
    let declared = deps(&[("alpha", "^1.0.0")]);
    let hash = compute_manifest_hash(&declared);
    let yaml = format!(
        "version: 1\nmanifest_hash: \"{hash}\"\ndeclared_dependencies:\n  alpha: \"^1.0.0\"\nworkers: {{}}\n",
    );
    let parsed = WorkerLockfile::from_yaml(&yaml).expect("consistent lock must validate");
    assert_eq!(parsed.manifest_hash.unwrap(), hash);
}

// ---------------------------------------------------------------------------
// 5. is_valid_manifest_hash
// ---------------------------------------------------------------------------

/// **Bug fix**: `compute_manifest_hash` always emits lowercase hex. A
/// lock with uppercase-hex `manifest_hash` would always trigger
/// false-positive drift on every sync. Reject at validation.
#[test]
fn is_valid_manifest_hash_rejects_uppercase() {
    let upper = format!("{MANIFEST_HASH_PREFIX}{}", "ABCDEF0123456789".repeat(4));
    assert!(
        !is_valid_manifest_hash(&upper),
        "uppercase hex must be rejected for byte-exact comparison"
    );
}

/// Lowercase hex remains accepted (sanity).
#[test]
fn is_valid_manifest_hash_accepts_lowercase() {
    let lower = format!("{MANIFEST_HASH_PREFIX}{}", "abcdef0123456789".repeat(4));
    assert!(is_valid_manifest_hash(&lower));
}

/// `compute_manifest_hash` of a single-entry dep is the SHA-256 of the
/// 12-byte string `"alpha=^1.0\n"` with the `sha256:v1:` prefix.
#[test]
fn compute_manifest_hash_byte_stable_pin() {
    let h = compute_manifest_hash(&deps(&[("alpha", "^1.0")]));
    let expected_hex = {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(b"alpha=^1.0\n");
        hex::encode(hasher.finalize())
    };
    assert_eq!(h, format!("{MANIFEST_HASH_PREFIX}{expected_hex}"));
}

/// Prefix is case-sensitive: `Sha256:v1:` (capital S) is rejected.
#[test]
fn is_valid_manifest_hash_prefix_is_case_sensitive() {
    let bad = format!("Sha256:v1:{}", "0".repeat(64));
    assert!(!is_valid_manifest_hash(&bad));
}

/// Unicode lookalikes for hex digits are rejected.
#[test]
fn manifest_hash_rejects_unicode_lookalike_hex() {
    let unicode_zero = "\u{0966}";
    let crafted = format!("{MANIFEST_HASH_PREFIX}{}", unicode_zero.repeat(64));
    assert!(!is_valid_manifest_hash(&crafted));
}
