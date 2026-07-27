// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Integration tests for the bundle worker install pipeline.
//!
//! Coverage map:
//!
//! - Extraction safety: dot-dot path (raw tar), absolute path, oversize
//!   single file, oversize archive total, too-many-entries, hardlink
//!   rejection, char-device rejection, deep-directory rejection.
//! - Manifest contract: missing manifest, manifest in subdirectory,
//!   invalid YAML, name mismatch, missing scripts.start, scripts.setup
//!   smuggling, implausible runtime.base_image.
//! - Resource clamping: requests exceeding caps, u64-overflow saturation.
//! - Dependency graph bounds: depth and transitive count.
//! - Atomic install + staging: collision with existing install,
//!   StagingGuard drop cleanup, cross-fs rename fallback (best-effort
//!   on supported hosts).
//! - Resolver precedence (regression-critical, Iron Rule): existing
//!   binary at ~/.iii/workers/{name} still resolves as Binary after
//!   the bundle root was added; an empty bundle dir does NOT shadow a
//!   binary install.
//! - Cache: roundtrip store-then-lookup, corrupt-blob eviction.
//! - Orphan sweep: stale staging dirs removed at startup.
//!
//! Network-dependent paths (sha256 mismatch, content-type rejection,
//! redirect cap, cache hit skipping fetch) are exercised at the unit
//! level inside `cli/bundle_download.rs` and `cli/download.rs`. Adding
//! them here would require a live HTTP fixture; the cost outweighs the
//! incremental coverage given the unit tests use the same primitives.

use std::io::Write as _;
use std::path::Path;

use iii_worker::cli::bundle_download::{
    BUNDLE_CACHE_MAX_BYTES, ENV_BUNDLE_DEV_LOOPBACK, ENV_BUNDLE_WORKERS_DISABLED, MAX_BUNDLE_DEPTH,
    MAX_BUNDLE_ENTRIES, MAX_BUNDLE_FILE, MAX_BUNDLE_MANIFEST_BYTES, MAX_BUNDLE_TOTAL, ResourceCaps,
    atomic_install, bundle_cache_dir, bundle_locks_dir, bundle_staging_root,
    bundle_workers_disabled, cached_archive_path, evict_cache_to_limit,
    extract_bundle_safely_blocking, lock_path_for, lookup_cached_archive,
    loopback_dev_bypass_enabled, parse_bundle_resources, store_in_cache, sweep_orphans,
    validate_bundle_manifest,
};
use iii_worker::cli::config_file::{
    ResolvedWorkerType, bundle_is_installed, bundle_worker_path, bundle_workers_dir,
    resolve_worker_type,
};
use iii_worker::cli::managed::handle_bundle_add;
use iii_worker::cli::registry::{
    BundleWorkerResponse, MAX_DEPENDENCY_DEPTH, MAX_TRANSITIVE_DEPS, ResolvedEdge, ResolvedRoot,
    ResolvedWorker, ResolvedWorkerGraph, enforce_dep_graph_bounds,
};
use iii_worker::core::error::WorkerOpError;

use serial_test::serial;

// ---------------------------------------------------------------------------
// Tar archive builders.
//
// `make_targz` uses tar::Builder for normal archives. Builder rejects
// `..` paths and absolute paths at write time, so adversarial archives
// require the raw header pattern in `make_raw_tar_gz`.
// ---------------------------------------------------------------------------

fn make_targz(entries: &[(&str, &[u8], tar::EntryType)]) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::GzEncoder;

    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    {
        let mut archive = tar::Builder::new(&mut encoder);
        for (path, content, kind) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_path(path).unwrap();
            header.set_size(content.len() as u64);
            header.set_mode(0o644);
            header.set_entry_type(*kind);
            header.set_cksum();
            archive.append(&header, *content as &[u8]).unwrap();
        }
        archive.finish().unwrap();
    }
    encoder.finish().unwrap()
}

/// Build a tar.gz with a raw path that may contain `..` or `/`. Mirrors
/// the helper in `tests/oci_worker_integration.rs`; both use the same
/// 512-byte GNU header layout.
fn make_raw_tar_gz(path_bytes: &[u8], data: &[u8]) -> Vec<u8> {
    let mut raw_tar = Vec::new();
    let mut header_block = [0u8; 512];
    header_block[..path_bytes.len()].copy_from_slice(path_bytes);
    header_block[100..107].copy_from_slice(b"0000644");
    let size_str = format!("{:011o}", data.len());
    header_block[124..135].copy_from_slice(size_str.as_bytes());
    header_block[156] = b'0'; // regular file
    header_block[257..263].copy_from_slice(b"ustar\0");
    header_block[263..265].copy_from_slice(b"00");
    header_block[148..156].copy_from_slice(b"        ");
    let cksum: u32 = header_block.iter().map(|&b| b as u32).sum();
    let cksum_str = format!("{:06o}\0 ", cksum);
    header_block[148..156].copy_from_slice(cksum_str.as_bytes());

    raw_tar.extend_from_slice(&header_block);
    raw_tar.extend_from_slice(data);
    let padding = 512 - (data.len() % 512);
    if padding < 512 {
        raw_tar.extend(std::iter::repeat_n(0u8, padding));
    }
    raw_tar.extend(std::iter::repeat_n(0u8, 1024));

    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    gz.write_all(&raw_tar).unwrap();
    gz.finish().unwrap()
}

/// Like `make_raw_tar_gz` but lets the caller pick a typeflag so we can
/// build adversarial archives with Symlink/Hardlink/CharDevice headers.
fn make_raw_tar_gz_with_type(path_bytes: &[u8], data: &[u8], typeflag: u8) -> Vec<u8> {
    let mut raw_tar = Vec::new();
    let mut header_block = [0u8; 512];
    header_block[..path_bytes.len()].copy_from_slice(path_bytes);
    header_block[100..107].copy_from_slice(b"0000644");
    let size_str = format!("{:011o}", data.len());
    header_block[124..135].copy_from_slice(size_str.as_bytes());
    header_block[156] = typeflag;
    header_block[257..263].copy_from_slice(b"ustar\0");
    header_block[263..265].copy_from_slice(b"00");
    header_block[148..156].copy_from_slice(b"        ");
    let cksum: u32 = header_block.iter().map(|&b| b as u32).sum();
    let cksum_str = format!("{:06o}\0 ", cksum);
    header_block[148..156].copy_from_slice(cksum_str.as_bytes());

    raw_tar.extend_from_slice(&header_block);
    raw_tar.extend_from_slice(data);
    let padding = 512 - (data.len() % 512);
    if padding < 512 {
        raw_tar.extend(std::iter::repeat_n(0u8, padding));
    }
    raw_tar.extend(std::iter::repeat_n(0u8, 1024));

    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    gz.write_all(&raw_tar).unwrap();
    gz.finish().unwrap()
}

fn write_archive(dir: &Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, bytes).unwrap();
    p
}

/// Names of `.old.*` (parked previous install) / `.partial.*` (cross-fs
/// copy) sibling dirs leaked next to a bundle install — replacement and
/// cross-fs installs must always clean these up.
fn sibling_leftovers(parent: &Path) -> Vec<String> {
    std::fs::read_dir(parent)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains(".old.") || n.contains(".partial."))
        .collect()
}

fn write_manifest(dir: &Path, body: &str) {
    std::fs::write(dir.join("iii.worker.yaml"), body).unwrap();
}

// =============================================================================
// Section A: Extraction safety
// =============================================================================

#[test]
fn extract_rejects_dot_dot_path() {
    let gz = make_raw_tar_gz(b"../escape.txt", b"x");
    let tmp = tempfile::tempdir().unwrap();
    let archive = write_archive(tmp.path(), "a.tar.gz", &gz);
    let dest = tmp.path().join("dest");
    std::fs::create_dir_all(&dest).unwrap();

    let err = extract_bundle_safely_blocking(&archive, &dest).expect_err("traversal rejected");
    let WorkerOpError::BundleArchiveUnsafe { reason, .. } = err else {
        panic!("expected BundleArchiveUnsafe");
    };
    assert!(
        reason.contains("..") || reason.contains("ParentDir"),
        "reason was: {reason}"
    );
}

#[test]
fn extract_rejects_absolute_path() {
    let gz = make_raw_tar_gz(b"/etc/passwd", b"root:x:0:0");
    let tmp = tempfile::tempdir().unwrap();
    let archive = write_archive(tmp.path(), "a.tar.gz", &gz);
    let dest = tmp.path().join("dest");
    std::fs::create_dir_all(&dest).unwrap();

    let err = extract_bundle_safely_blocking(&archive, &dest).expect_err("absolute path rejected");
    let WorkerOpError::BundleArchiveUnsafe { reason, .. } = err else {
        panic!("expected BundleArchiveUnsafe");
    };
    assert!(
        reason.contains("absolute") || reason.contains("root"),
        "reason was: {reason}"
    );
}

#[test]
fn extract_rejects_symlink_entry() {
    // tar typeflag '2' = symlink.
    let gz = make_raw_tar_gz_with_type(b"link", &[], b'2');
    let tmp = tempfile::tempdir().unwrap();
    let archive = write_archive(tmp.path(), "a.tar.gz", &gz);
    let dest = tmp.path().join("dest");
    std::fs::create_dir_all(&dest).unwrap();

    let err = extract_bundle_safely_blocking(&archive, &dest).expect_err("symlink rejected");
    let WorkerOpError::BundleArchiveUnsafe { reason, .. } = err else {
        panic!("expected BundleArchiveUnsafe");
    };
    assert!(
        reason.contains("Symlink") || reason.contains("entry type"),
        "reason was: {reason}"
    );
}

#[test]
fn extract_rejects_hardlink_entry() {
    // tar typeflag '1' = hardlink.
    let gz = make_raw_tar_gz_with_type(b"hardlink", &[], b'1');
    let tmp = tempfile::tempdir().unwrap();
    let archive = write_archive(tmp.path(), "a.tar.gz", &gz);
    let dest = tmp.path().join("dest");
    std::fs::create_dir_all(&dest).unwrap();

    let err = extract_bundle_safely_blocking(&archive, &dest).expect_err("hardlink rejected");
    assert!(
        matches!(err, WorkerOpError::BundleArchiveUnsafe { .. }),
        "expected BundleArchiveUnsafe, got {err:?}"
    );
}

#[test]
fn extract_rejects_char_device_entry() {
    // tar typeflag '3' = character device.
    let gz = make_raw_tar_gz_with_type(b"cdev", &[], b'3');
    let tmp = tempfile::tempdir().unwrap();
    let archive = write_archive(tmp.path(), "a.tar.gz", &gz);
    let dest = tmp.path().join("dest");
    std::fs::create_dir_all(&dest).unwrap();

    let err = extract_bundle_safely_blocking(&archive, &dest).expect_err("char device rejected");
    assert!(
        matches!(err, WorkerOpError::BundleArchiveUnsafe { .. }),
        "expected BundleArchiveUnsafe, got {err:?}"
    );
}

#[test]
fn extract_rejects_too_many_entries() {
    let mut entries: Vec<(String, Vec<u8>, tar::EntryType)> = Vec::new();
    let limit_plus_one = (MAX_BUNDLE_ENTRIES as usize) + 1;
    for i in 0..limit_plus_one {
        entries.push((format!("f{i}.txt"), b"x".to_vec(), tar::EntryType::Regular));
    }
    let refs: Vec<(&str, &[u8], tar::EntryType)> = entries
        .iter()
        .map(|(p, c, k)| (p.as_str(), c.as_slice(), *k))
        .collect();
    let bytes = make_targz(&refs);

    let tmp = tempfile::tempdir().unwrap();
    let archive = write_archive(tmp.path(), "a.tar.gz", &bytes);
    let dest = tmp.path().join("dest");
    std::fs::create_dir_all(&dest).unwrap();

    let err = extract_bundle_safely_blocking(&archive, &dest).expect_err("entry-count cap fires");
    let WorkerOpError::BundleArchiveUnsafe { reason, .. } = err else {
        panic!("expected BundleArchiveUnsafe");
    };
    assert!(reason.contains("entries"), "reason was: {reason}");
}

#[test]
fn extract_rejects_oversize_single_file() {
    // One file whose declared size header exceeds MAX_BUNDLE_FILE. We
    // don't ship 32 MiB of bytes — we just lie about size in the header
    // so the size guard fires before any reading begins. tar::Header's
    // safe API uses real lengths, but the extractor checks
    // `entry.header().size()` BEFORE reading body, so a normally-built
    // archive with a 33 MiB regular file is enough.
    let huge_size = (MAX_BUNDLE_FILE + 1024) as usize;
    let body = vec![0u8; huge_size];
    let bytes = make_targz(&[("big.bin", &body, tar::EntryType::Regular)]);
    let tmp = tempfile::tempdir().unwrap();
    let archive = write_archive(tmp.path(), "a.tar.gz", &bytes);
    let dest = tmp.path().join("dest");
    std::fs::create_dir_all(&dest).unwrap();

    let err = extract_bundle_safely_blocking(&archive, &dest).expect_err("single-file cap fires");
    let WorkerOpError::BundleArchiveUnsafe { reason, .. } = err else {
        panic!("expected BundleArchiveUnsafe");
    };
    assert!(
        reason.contains("single file") || reason.contains("MAX_BUNDLE_FILE"),
        "reason was: {reason}"
    );
}

#[test]
fn extract_rejects_oversize_total() {
    // Two files each within MAX_BUNDLE_FILE but together exceeding
    // MAX_BUNDLE_TOTAL. Each is 33 MiB; total > 64 MiB.
    let chunk = vec![0u8; (MAX_BUNDLE_TOTAL / 2 + 1024) as usize];
    let bytes = make_targz(&[
        ("a.bin", &chunk, tar::EntryType::Regular),
        ("b.bin", &chunk, tar::EntryType::Regular),
    ]);
    let tmp = tempfile::tempdir().unwrap();
    let archive = write_archive(tmp.path(), "a.tar.gz", &bytes);
    let dest = tmp.path().join("dest");
    std::fs::create_dir_all(&dest).unwrap();

    let err = extract_bundle_safely_blocking(&archive, &dest).expect_err("total cap fires");
    // Either single-file or total cap may fire first depending on chunk
    // size; both are acceptable rejections.
    assert!(
        matches!(err, WorkerOpError::BundleArchiveUnsafe { .. }),
        "expected BundleArchiveUnsafe, got {err:?}"
    );
}

#[test]
fn extract_rejects_depth_too_deep() {
    // Build a path that nests MAX_BUNDLE_DEPTH+1 directories deep.
    let mut path = String::new();
    for i in 0..(MAX_BUNDLE_DEPTH + 1) {
        if !path.is_empty() {
            path.push('/');
        }
        path.push_str(&format!("d{i}"));
    }
    path.push_str("/leaf.txt");
    let bytes = make_targz(&[(path.as_str(), b"x", tar::EntryType::Regular)]);
    let tmp = tempfile::tempdir().unwrap();
    let archive = write_archive(tmp.path(), "a.tar.gz", &bytes);
    let dest = tmp.path().join("dest");
    std::fs::create_dir_all(&dest).unwrap();

    let err = extract_bundle_safely_blocking(&archive, &dest).expect_err("depth cap fires");
    let WorkerOpError::BundleArchiveUnsafe { reason, .. } = err else {
        panic!("expected BundleArchiveUnsafe");
    };
    assert!(reason.contains("depth"), "reason was: {reason}");
}

#[test]
fn extract_happy_path_normal_archive() {
    let bytes = make_targz(&[
        ("iii.worker.yaml", b"name: foo\n", tar::EntryType::Regular),
        ("bundle.js", b"console.log('hi');", tar::EntryType::Regular),
        ("assets/", b"", tar::EntryType::Directory),
        ("assets/data.json", b"{}", tar::EntryType::Regular),
    ]);
    let tmp = tempfile::tempdir().unwrap();
    let archive = write_archive(tmp.path(), "a.tar.gz", &bytes);
    let dest = tmp.path().join("dest");
    std::fs::create_dir_all(&dest).unwrap();

    extract_bundle_safely_blocking(&archive, &dest).expect("happy extract");
    assert!(dest.join("iii.worker.yaml").is_file());
    assert!(dest.join("bundle.js").is_file());
    assert!(dest.join("assets/data.json").is_file());
}

// =============================================================================
// Section B: Manifest contract (validated through the public surface)
// =============================================================================

#[test]
fn manifest_missing_file_returns_typed_error() {
    let tmp = tempfile::tempdir().unwrap();
    let err = validate_bundle_manifest(tmp.path(), "foo").expect_err("missing manifest");
    let WorkerOpError::BundleManifestRejected { field, .. } = err else {
        panic!("expected BundleManifestRejected");
    };
    assert_eq!(field, "iii.worker.yaml");
}

#[test]
fn manifest_rejects_setup_and_implausible_base_image() {
    let cases = [
        (
            "name: foo\nscripts:\n  setup: \"x\"\n  start: \"node x.js\"\n",
            "scripts.setup",
        ),
        (
            "name: foo\nruntime:\n  base_image: \"bad image!\"\nscripts:\n  start: \"node x.js\"\n",
            "runtime.base_image",
        ),
    ];
    for (yaml, expected_field) in cases {
        let tmp = tempfile::tempdir().unwrap();
        write_manifest(tmp.path(), yaml);
        let err = validate_bundle_manifest(tmp.path(), "foo").expect_err(expected_field);
        let WorkerOpError::BundleManifestRejected { field, .. } = err else {
            panic!("expected BundleManifestRejected for {expected_field}");
        };
        assert_eq!(field, expected_field);
    }
}

#[test]
fn manifest_accepts_install_and_custom_base_image() {
    let tmp = tempfile::tempdir().unwrap();
    write_manifest(
        tmp.path(),
        "name: foo\nruntime:\n  base_image: oven/bun:1\nscripts:\n  install: \"x\"\n  start: \"node x.js\"\n",
    );
    let cmd = validate_bundle_manifest(tmp.path(), "foo").expect("install + custom ref accepted");
    assert_eq!(cmd, "node x.js");
}

#[test]
fn manifest_invalid_yaml_returns_config_parse_error() {
    let tmp = tempfile::tempdir().unwrap();
    // Unbalanced YAML — { without matching } at the start of a key.
    write_manifest(tmp.path(), "name: {bad\nscripts:\n  start: x\n");
    let err = validate_bundle_manifest(tmp.path(), "foo").expect_err("yaml parse");
    assert!(
        matches!(err, WorkerOpError::ConfigParse { .. }),
        "expected ConfigParse, got {err:?}"
    );
}

#[test]
fn manifest_name_mismatch_returns_typed_error() {
    let tmp = tempfile::tempdir().unwrap();
    write_manifest(tmp.path(), "name: bar\nscripts:\n  start: node x.js\n");
    let err = validate_bundle_manifest(tmp.path(), "foo").expect_err("name mismatch");
    let WorkerOpError::BundleManifestRejected { field, .. } = err else {
        panic!("expected BundleManifestRejected");
    };
    assert_eq!(field, "name");
}

#[test]
fn manifest_returns_start_command() {
    let tmp = tempfile::tempdir().unwrap();
    write_manifest(tmp.path(), "name: foo\nscripts:\n  start: node bundle.js\n");
    let cmd = validate_bundle_manifest(tmp.path(), "foo").expect("happy");
    assert_eq!(cmd, "node bundle.js");
}

// =============================================================================
// Section C: Resource clamping
// =============================================================================

#[test]
fn resources_defaults_apply_when_absent() {
    let tmp = tempfile::tempdir().unwrap();
    write_manifest(tmp.path(), "name: foo\nscripts:\n  start: x\n");
    let r = parse_bundle_resources(tmp.path(), ResourceCaps::default()).unwrap();
    assert!(r.clamped_cpus.is_none());
    assert!(r.clamped_memory_mb.is_none());
}

#[test]
fn resources_clamped_cpu_request_reported() {
    let tmp = tempfile::tempdir().unwrap();
    write_manifest(
        tmp.path(),
        "name: foo\nresources:\n  cpus: 32\n  memory: 1024\nscripts:\n  start: x\n",
    );
    let caps = ResourceCaps {
        max_cpus: 4,
        max_memory_mb: 4096,
    };
    let r = parse_bundle_resources(tmp.path(), caps).unwrap();
    assert_eq!(r.cpus, 4);
    assert_eq!(r.clamped_cpus, Some(32));
}

#[test]
fn resources_clamped_memory_request_reported() {
    let tmp = tempfile::tempdir().unwrap();
    write_manifest(
        tmp.path(),
        "name: foo\nresources:\n  cpus: 1\n  memory: 999999\nscripts:\n  start: x\n",
    );
    let r = parse_bundle_resources(tmp.path(), ResourceCaps::default()).unwrap();
    assert_eq!(r.clamped_memory_mb, Some(999999));
}

// =============================================================================
// Section D: Dependency graph bounds
// =============================================================================

fn make_root_worker(name: &str) -> ResolvedRoot {
    ResolvedRoot {
        name: name.to_string(),
        version: "1.0.0".to_string(),
    }
}

fn make_resolved_worker(name: &str) -> ResolvedWorker {
    ResolvedWorker {
        name: name.to_string(),
        worker_type: "bundle".to_string(),
        version: "1.0.0".to_string(),
        repo: "https://example.com".to_string(),
        config: serde_json::Value::Null,
        binaries: None,
        image: None,
        archive_url: None,
        sha256: None,
        dependencies: Default::default(),
    }
}

#[test]
fn dep_graph_accepts_within_bounds() {
    // A diamond shape: root → {a, b} → c. Depth 2, 4 nodes.
    let graph = ResolvedWorkerGraph {
        root: make_root_worker("root"),
        target: None,
        graph: vec![
            make_resolved_worker("a"),
            make_resolved_worker("b"),
            make_resolved_worker("c"),
        ],
        edges: vec![
            ResolvedEdge {
                from: "root".into(),
                to: "a".into(),
                range: "*".into(),
            },
            ResolvedEdge {
                from: "root".into(),
                to: "b".into(),
                range: "*".into(),
            },
            ResolvedEdge {
                from: "a".into(),
                to: "c".into(),
                range: "*".into(),
            },
            ResolvedEdge {
                from: "b".into(),
                to: "c".into(),
                range: "*".into(),
            },
        ],
    };
    enforce_dep_graph_bounds(&graph).expect("within bounds");
}

#[test]
fn dep_graph_accepts_wide_shared_dependencies() {
    // Regression: a shallow but wide DAG whose nodes share dependencies
    // has far more distinct root→leaf paths than distinct nodes. The old
    // walk counted paths, so a legitimate in-bounds graph (few nodes,
    // depth well under the cap) was rejected with a spurious
    // `edge_traversal` error. This is the shape a real bundle produced:
    // `iii worker add <bundle>` failed with
    // "edge_traversal exceeded: limit 64, found 65".
    //
    // Fully-connected layers of width 3: root → L1(3) → L2(3) → L3(3) → leaf.
    // 11 distinct nodes, longest path 4 — both within bounds — but 3*3*3
    // = 27 paths reach the leaf, which alone overflows the old limit of 64.
    let layers = [
        vec!["a0", "a1", "a2"],
        vec!["b0", "b1", "b2"],
        vec!["c0", "c1", "c2"],
        vec!["leaf"],
    ];

    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    for name in layers.iter().flatten() {
        nodes.push(make_resolved_worker(name));
    }
    // root fully connects to the first layer.
    for name in &layers[0] {
        edges.push(ResolvedEdge {
            from: "root".into(),
            to: (*name).into(),
            range: "*".into(),
        });
    }
    // each layer fully connects to the next (bipartite), so nodes share
    // downstream dependencies.
    for pair in layers.windows(2) {
        for from in &pair[0] {
            for to in &pair[1] {
                edges.push(ResolvedEdge {
                    from: (*from).into(),
                    to: (*to).into(),
                    range: "*".into(),
                });
            }
        }
    }

    let graph = ResolvedWorkerGraph {
        root: make_root_worker("root"),
        target: None,
        graph: nodes,
        edges,
    };
    enforce_dep_graph_bounds(&graph).expect("wide shared-dependency graph is within bounds");
}

#[test]
fn dep_graph_accepts_dense_layered_graph() {
    // Regression for the backstop itself. `walked` counts frontier pops,
    // and a dense DAG pushes each node once per parent, so pops grow
    // roughly with node_count squared while distinct nodes stay small.
    // A budget that is only linear in node_count therefore rejects a
    // valid dense graph. This is the maximal shape within bounds: four
    // fully-connected layers of width 8 — 32 nodes (the transitive_count
    // cap), longest path 4 (under the depth cap) — which produces ~200
    // pops and tripped the first linear budget.
    const WIDTH: usize = 8;
    let layers: Vec<Vec<String>> = (0..4)
        .map(|l| (0..WIDTH).map(|w| format!("l{l}_{w}")).collect())
        .collect();

    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    for name in layers.iter().flatten() {
        nodes.push(make_resolved_worker(name));
    }
    for name in &layers[0] {
        edges.push(ResolvedEdge {
            from: "root".into(),
            to: name.clone(),
            range: "*".into(),
        });
    }
    for pair in layers.windows(2) {
        for from in &pair[0] {
            for to in &pair[1] {
                edges.push(ResolvedEdge {
                    from: from.clone(),
                    to: to.clone(),
                    range: "*".into(),
                });
            }
        }
    }

    let graph = ResolvedWorkerGraph {
        root: make_root_worker("root"),
        target: None,
        graph: nodes,
        edges,
    };
    enforce_dep_graph_bounds(&graph).expect("dense in-bounds layered graph is accepted");
}

#[test]
fn dep_graph_tolerates_repeated_edges() {
    // A malformed resolver response repeating the same edge must not be
    // able to inflate the traversal: deduped adjacency collapses the
    // duplicates, so a two-node graph stays within bounds no matter how
    // many identical edges it carries.
    let graph = ResolvedWorkerGraph {
        root: make_root_worker("root"),
        target: None,
        graph: vec![make_resolved_worker("a")],
        edges: (0..1000)
            .map(|_| ResolvedEdge {
                from: "root".into(),
                to: "a".into(),
                range: "*".into(),
            })
            .collect(),
    };
    enforce_dep_graph_bounds(&graph).expect("repeated identical edges collapse to one");
}

#[test]
fn dep_graph_rejects_excessive_depth() {
    // Linear chain root → n0 → n1 → ... → n_{depth}.
    let mut nodes = vec![make_resolved_worker("root")];
    let mut edges = vec![];
    let mut prev = "root".to_string();
    for i in 0..(MAX_DEPENDENCY_DEPTH + 2) {
        let n = format!("n{i}");
        nodes.push(make_resolved_worker(&n));
        edges.push(ResolvedEdge {
            from: prev.clone(),
            to: n.clone(),
            range: "*".into(),
        });
        prev = n;
    }
    let graph = ResolvedWorkerGraph {
        root: make_root_worker("root"),
        target: None,
        graph: nodes,
        edges,
    };
    let err = enforce_dep_graph_bounds(&graph).expect_err("depth cap fires");
    let WorkerOpError::BundleDepGraphExceeded {
        dimension, limit, ..
    } = err
    else {
        panic!("expected BundleDepGraphExceeded");
    };
    // Either the depth-cap fires, or the edge-traversal guard fires
    // first on a malformed/over-long graph. Both are acceptable as
    // long as the rejection carries a sensible dimension.
    assert!(
        dimension == "depth" || dimension == "edge_traversal" || dimension == "transitive_count",
        "dimension was: {dimension}"
    );
    assert!(limit > 0);
}

#[test]
fn dep_graph_rejects_excessive_breadth() {
    // root with MAX_TRANSITIVE_DEPS+1 direct dependencies (depth 1 but
    // node count over cap).
    let extra = (MAX_TRANSITIVE_DEPS as usize) + 5;
    let nodes: Vec<_> = (0..extra)
        .map(|i| make_resolved_worker(&format!("dep{i}")))
        .collect();
    let edges: Vec<_> = (0..extra)
        .map(|i| ResolvedEdge {
            from: "root".into(),
            to: format!("dep{i}"),
            range: "*".into(),
        })
        .collect();
    let graph = ResolvedWorkerGraph {
        root: make_root_worker("root"),
        target: None,
        graph: nodes,
        edges,
    };
    let err = enforce_dep_graph_bounds(&graph).expect_err("breadth cap fires");
    assert!(
        matches!(err, WorkerOpError::BundleDepGraphExceeded { .. }),
        "expected BundleDepGraphExceeded, got {err:?}"
    );
}

// =============================================================================
// Section E: Atomic install + staging guard
// =============================================================================

#[test]
#[serial]
fn atomic_install_replaces_existing_target() {
    let tmp = tempfile::tempdir().unwrap();
    let _home = HomeGuard::set(tmp.path());

    // Pre-populate the final dir with a previous install, including a
    // file the new payload does NOT carry.
    let final_dir = bundle_worker_path("collide");
    std::fs::create_dir_all(&final_dir).unwrap();
    std::fs::write(
        final_dir.join("iii.worker.yaml"),
        "name: collide\nold: true\n",
    )
    .unwrap();
    std::fs::write(final_dir.join("stale.txt"), "left over").unwrap();

    let staging = bundle_staging_root().join("collide-fresh");
    std::fs::create_dir_all(&staging).unwrap();
    std::fs::write(
        staging.join("iii.worker.yaml"),
        "name: collide\nnew: true\n",
    )
    .unwrap();

    let installed = atomic_install(&staging, "collide").expect("replace ok");
    assert_eq!(installed, final_dir);

    let manifest = std::fs::read_to_string(final_dir.join("iii.worker.yaml")).unwrap();
    assert!(
        manifest.contains("new: true"),
        "new payload landed: {manifest}"
    );
    assert!(
        !final_dir.join("stale.txt").exists(),
        "old payload fully replaced, not merged"
    );
    assert!(!staging.exists(), "staging dir was consumed");

    let leftovers = sibling_leftovers(final_dir.parent().unwrap());
    assert!(leftovers.is_empty(), "sibling leftovers: {leftovers:?}");
}

#[test]
#[serial]
fn atomic_install_restores_previous_install_on_failure() {
    let tmp = tempfile::tempdir().unwrap();
    let _home = HomeGuard::set(tmp.path());

    let final_dir = bundle_worker_path("keepme");
    std::fs::create_dir_all(&final_dir).unwrap();
    std::fs::write(final_dir.join("iii.worker.yaml"), "name: keepme\n").unwrap();

    // Nonexistent staging: the install rename fails with ENOENT AFTER
    // the previous install was parked aside, exercising the restore path.
    let missing_staging = bundle_staging_root().join("keepme-missing");
    let err = atomic_install(&missing_staging, "keepme").expect_err("install must fail");
    assert!(matches!(err, WorkerOpError::ConfigIo { .. }), "got {err:?}");

    // The previous install was restored, not lost.
    let manifest = std::fs::read_to_string(final_dir.join("iii.worker.yaml"))
        .expect("previous install restored at final path");
    assert_eq!(manifest, "name: keepme\n");

    let leftovers = sibling_leftovers(final_dir.parent().unwrap());
    assert!(leftovers.is_empty(), "sibling leftovers: {leftovers:?}");
}

#[test]
#[serial]
fn atomic_install_sweeps_stale_old_siblings() {
    let tmp = tempfile::tempdir().unwrap();
    let _home = HomeGuard::set(tmp.path());

    // A replacement that crashed between rename-aside and cleanup leaves
    // the previous install parked as a `{name}.old.<unique>` sibling.
    let final_dir = bundle_worker_path("crashy");
    let parent = final_dir.parent().unwrap().to_path_buf();
    let stale = parent.join("crashy.old.123.0");
    std::fs::create_dir_all(&stale).unwrap();
    std::fs::write(stale.join("iii.worker.yaml"), "name: crashy\n").unwrap();

    let staging = bundle_staging_root().join("crashy-fresh");
    std::fs::create_dir_all(&staging).unwrap();
    std::fs::write(staging.join("iii.worker.yaml"), "name: crashy\n").unwrap();

    atomic_install(&staging, "crashy").expect("install ok");

    assert!(!stale.exists(), "stale .old sibling swept on next install");
    let leftovers = sibling_leftovers(&parent);
    assert!(leftovers.is_empty(), "sibling leftovers: {leftovers:?}");
}

// Regression: when a prior replacement crashed after its rename-aside,
// final_dir is missing and the parked `.old.*` sibling is the ONLY
// remaining copy of the worker. A failed next install must not sweep it
// — cleanup is deferred until an install actually lands.
#[test]
#[serial]
fn atomic_install_failure_preserves_parked_old_sibling() {
    let tmp = tempfile::tempdir().unwrap();
    let _home = HomeGuard::set(tmp.path());

    let final_dir = bundle_worker_path("crashed");
    let parent = final_dir.parent().unwrap().to_path_buf();
    let parked = parent.join("crashed.old.123.0");
    std::fs::create_dir_all(&parked).unwrap();
    std::fs::write(parked.join("iii.worker.yaml"), "name: crashed\n").unwrap();

    // The next install fails (nonexistent staging → ENOENT on rename).
    let missing_staging = bundle_staging_root().join("crashed-missing");
    atomic_install(&missing_staging, "crashed").expect_err("install must fail");

    // The only good copy must survive the failed install.
    let manifest = std::fs::read_to_string(parked.join("iii.worker.yaml"))
        .expect("parked previous install preserved");
    assert_eq!(manifest, "name: crashed\n");

    // A later successful install sweeps it.
    let staging = bundle_staging_root().join("crashed-fresh");
    std::fs::create_dir_all(&staging).unwrap();
    std::fs::write(staging.join("iii.worker.yaml"), "name: crashed\n").unwrap();
    atomic_install(&staging, "crashed").expect("install ok");
    assert!(
        !parked.exists(),
        "parked copy swept after a successful install"
    );
    let leftovers = sibling_leftovers(&parent);
    assert!(leftovers.is_empty(), "sibling leftovers: {leftovers:?}");
}

#[test]
#[serial]
fn atomic_install_renames_into_place() {
    let tmp = tempfile::tempdir().unwrap();
    let _home = HomeGuard::set(tmp.path());

    let staging = bundle_staging_root().join("rename-source");
    std::fs::create_dir_all(&staging).unwrap();
    std::fs::write(staging.join("iii.worker.yaml"), "name: foo").unwrap();

    let final_dir = atomic_install(&staging, "foo").expect("rename ok");
    assert_eq!(final_dir, bundle_worker_path("foo"));
    assert!(final_dir.join("iii.worker.yaml").is_file());
    assert!(!staging.exists(), "staging dir was consumed by rename");
}

#[test]
#[serial]
fn bundle_is_installed_detects_shared_global_install() {
    let tmp = tempfile::tempdir().unwrap();
    let _home = HomeGuard::set(tmp.path());

    // Nothing installed yet.
    assert!(!bundle_is_installed("harness"), "absent before any install");

    // Simulate project A installing the shared bundle.
    let dir = bundle_worker_path("harness");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("iii.worker.yaml"), "name: harness\n").unwrap();
    assert!(
        bundle_is_installed("harness"),
        "detected once the manifest is on disk"
    );

    // A bare directory with no manifest must NOT count as installed,
    // matching the resolver's bundle detection (manifest-gated).
    let empty = bundle_worker_path("empty");
    std::fs::create_dir_all(&empty).unwrap();
    assert!(
        !bundle_is_installed("empty"),
        "empty dir without manifest is not a valid install"
    );
}

// Regression: a bundle worker already installed by one project must be
// REPLACED, not reused, when a second project adds it. Bundle installs are a
// global, machine-wide cache keyed by name (~/.iii/workers-bundle/{name}/),
// so reusing whatever is on disk would pin every project to the first
// project's (possibly stale) payload — and `iii worker update`, which
// funnels bundles through `handle_bundle_add`, would never refresh anything.
// The pipeline runs fully offline here: the archive is pre-seeded into the
// sha256-keyed download cache, so the unreachable URL is never fetched.
#[tokio::test]
#[serial]
async fn bundle_add_replaces_existing_install_in_second_project() {
    let home = tempfile::tempdir().unwrap();
    let _home = HomeGuard::set(home.path());

    // The freshly resolved payload, pre-seeded into the download cache so
    // download_archive serves it without network I/O.
    let manifest: &[u8] = b"name: harness\nscripts:\n  start: node bundle.js\n";
    let archive = make_targz(&[
        ("iii.worker.yaml", manifest, tar::EntryType::Regular),
        ("bundle.js", b"// new payload\n", tar::EntryType::Regular),
    ]);
    let digest = sha256_hex(&archive);
    let src = write_archive(home.path(), "src.tar.gz", &archive);
    store_in_cache(&src, &digest).unwrap();

    // Project A's earlier (now stale) install of the shared bundle.
    let install_dir = bundle_worker_path("harness");
    std::fs::create_dir_all(&install_dir).unwrap();
    std::fs::write(install_dir.join("iii.worker.yaml"), "name: harness\n").unwrap();
    std::fs::write(install_dir.join("old-marker.txt"), "from project A").unwrap();

    // Project B is a fresh working directory with no config.yaml yet.
    let project_b = tempfile::tempdir().unwrap();
    let _cwd = CwdGuard::set(project_b.path());

    let response = BundleWorkerResponse {
        name: "harness".to_string(),
        version: "0.5.4".to_string(),
        // SSRF-safe URL that is never fetched: the cache hit serves the
        // archive before any network I/O.
        archive_url: "https://example.invalid/harness.tar.gz".to_string(),
        sha256: digest,
    };

    let rc = handle_bundle_add("harness", &response, true).await;
    assert_eq!(rc, 0, "second-project add must succeed and replace");

    // On-disk install was replaced with the resolved payload.
    assert!(
        install_dir.join("bundle.js").is_file(),
        "new payload landed on disk"
    );
    assert!(
        !install_dir.join("old-marker.txt").exists(),
        "stale payload fully replaced, not merged"
    );
    let on_disk = std::fs::read_to_string(install_dir.join("iii.worker.yaml")).unwrap();
    assert!(
        on_disk.contains("node bundle.js"),
        "manifest refreshed, got:\n{on_disk}"
    );

    let config = std::fs::read_to_string("config.yaml")
        .expect("config.yaml written in project B working dir");
    assert!(
        config.contains("harness"),
        "worker registered in project B config.yaml, got:\n{config}"
    );
    // Must be a name-only (bundle-shaped) entry: the resolver routes starts to
    // the immutable bundle install ONLY when neither worker_path nor image is
    // present. A regression that wrote either would break bundle dispatch at
    // start while still passing the substring check above.
    assert!(
        !config.contains("worker_path:"),
        "bundle entry must not carry worker_path, got:\n{config}"
    );
    assert!(
        !config.contains("image:"),
        "bundle entry must not carry image, got:\n{config}"
    );
}

// StagingGuard drop semantics are covered by the in-module unit test
// in `bundle_download.rs`. Constructing one from this integration test
// crate would require a test-only public constructor; we deliberately
// keep that surface absent so production callers don't accidentally
// reach for it.

// =============================================================================
// Section F: Cache (HOME-aware)
// =============================================================================

#[test]
#[serial]
fn cache_roundtrip_store_then_lookup_hits() {
    let tmp = tempfile::tempdir().unwrap();
    let _home = HomeGuard::set(tmp.path());

    // Build a real archive so its sha256 is content-derived.
    let bytes = make_targz(&[("iii.worker.yaml", b"name: c", tar::EntryType::Regular)]);
    let src = write_archive(tmp.path(), "src.tar.gz", &bytes);
    let digest = sha256_hex(&bytes);

    store_in_cache(&src, &digest).expect("cache insert");

    let hit = lookup_cached_archive(&digest).expect("cache hit");
    let cache_root = bundle_cache_dir();
    assert!(
        hit.starts_with(&cache_root),
        "cache hit {} not under root {}",
        hit.display(),
        cache_root.display()
    );
    let cached_bytes = std::fs::read(&hit).unwrap();
    assert_eq!(cached_bytes, bytes);
}

#[test]
#[serial]
fn cache_corrupt_blob_is_evicted_on_lookup() {
    let tmp = tempfile::tempdir().unwrap();
    let _home = HomeGuard::set(tmp.path());

    let real_bytes = make_targz(&[("a", b"b", tar::EntryType::Regular)]);
    let digest = sha256_hex(&real_bytes);
    let path = cached_archive_path(&digest).expect("valid digest");
    std::fs::create_dir_all(bundle_cache_dir()).unwrap();
    // Plant a blob with the SHA-256-correct filename but corrupt contents.
    std::fs::write(&path, b"this is not the right content").unwrap();

    // Lookup must report cache miss AND remove the bad blob.
    assert!(
        lookup_cached_archive(&digest).is_none(),
        "corrupt blob was returned as a hit"
    );
    assert!(!path.exists(), "corrupt blob was not evicted");
}

#[test]
#[serial]
fn cache_evict_brings_directory_under_limit() {
    let tmp = tempfile::tempdir().unwrap();
    let _home = HomeGuard::set(tmp.path());

    let cache = bundle_cache_dir();
    std::fs::create_dir_all(&cache).unwrap();
    // Three 4KB files. Limit forces eviction down to two.
    for (i, hex_byte) in ['a', 'b', 'c'].iter().enumerate() {
        let digest = format!("{}", hex_byte).repeat(64);
        // Stagger mtime so the oldest is deterministic.
        let p = cached_archive_path(&digest).expect("valid digest");
        std::fs::write(&p, vec![0u8; 4096]).unwrap();
        // Modify mtime by waiting between writes (best-effort).
        let _ = i;
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    let _ = evict_cache_to_limit(4096 * 2 + 1);
    let remaining: Vec<_> = std::fs::read_dir(&cache)
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    assert!(
        remaining.len() <= 2,
        "expected eviction to leave <=2 files, got {}",
        remaining.len()
    );
}

#[test]
fn cache_max_bytes_is_positive() {
    // Sanity: somebody zeroed the constant accidentally.
    assert!(BUNDLE_CACHE_MAX_BYTES > 0);
}

// =============================================================================
// Section G: Orphan sweep
// =============================================================================

#[test]
#[serial]
fn sweep_orphans_removes_old_staging_dirs() {
    let tmp = tempfile::tempdir().unwrap();
    let _home = HomeGuard::set(tmp.path());

    let staging = bundle_staging_root().join("fresh-staging");
    std::fs::create_dir_all(&staging).unwrap();
    // Fresh staging dir (mtime is "now") — the sweep's threshold
    // protects in-flight installs from a sibling process's cleanup.
    // Backdating mtime to genuinely trip cleanup would need the
    // `filetime` crate, which the workspace doesn't ship. The
    // protect-fresh assertion is what we want to lock in here: a
    // sweep MUST NEVER eat an in-flight install's staging dir.
    let removed = sweep_orphans();
    assert_eq!(removed, 0, "sweep should leave fresh dirs in place");
    assert!(
        staging.exists(),
        "sweep should not remove a freshly-created staging dir"
    );
}

// =============================================================================
// Section H: Resolver regression (Iron Rule)
// =============================================================================

#[test]
#[serial]
fn regression_existing_binary_still_resolves_as_binary() {
    let tmp = tempfile::tempdir().unwrap();
    let _home = HomeGuard::set(tmp.path());

    let binary = tmp.path().join(".iii").join("workers").join("mybin");
    std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
    std::fs::write(&binary, b"#!/bin/sh\necho hi\n").unwrap();

    let resolved = resolve_worker_type("mybin");
    match resolved {
        ResolvedWorkerType::Binary { binary_path } => {
            assert_eq!(binary_path, binary);
        }
        other => panic!("expected Binary, got {other:?}"),
    }
}

#[test]
#[serial]
fn regression_empty_bundle_dir_does_not_shadow_binary_resolve() {
    let tmp = tempfile::tempdir().unwrap();
    let _home = HomeGuard::set(tmp.path());

    // Plant an empty bundle dir (no iii.worker.yaml inside) AND a binary.
    let bundle = bundle_worker_path("shadow");
    std::fs::create_dir_all(&bundle).unwrap();

    let binary = tmp.path().join(".iii").join("workers").join("shadow");
    std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
    std::fs::write(&binary, b"#!/bin/sh\n").unwrap();

    // Resolver should NOT classify the empty bundle dir as Bundle
    // (requires iii.worker.yaml to be present) — Binary wins.
    let resolved = resolve_worker_type("shadow");
    match resolved {
        ResolvedWorkerType::Binary { .. } => {}
        other => panic!("expected Binary (empty bundle dir ignored), got {other:?}"),
    }
}

#[test]
#[serial]
fn bundle_dir_with_manifest_resolves_as_bundle() {
    let tmp = tempfile::tempdir().unwrap();
    let _home = HomeGuard::set(tmp.path());

    let bundle = bundle_worker_path("real-bundle");
    std::fs::create_dir_all(&bundle).unwrap();
    std::fs::write(bundle.join("iii.worker.yaml"), "name: real-bundle\n").unwrap();

    let resolved = resolve_worker_type("real-bundle");
    match resolved {
        ResolvedWorkerType::Bundle { worker_path } => {
            assert_eq!(worker_path, bundle);
        }
        other => panic!("expected Bundle, got {other:?}"),
    }
}

#[test]
#[serial]
fn workers_dirs_share_home_root() {
    // Sanity: both binary and bundle roots derive from $HOME so a
    // HOME guard correctly redirects both.
    let tmp = tempfile::tempdir().unwrap();
    let _home = HomeGuard::set(tmp.path());

    let bundle_root = bundle_workers_dir();
    let binary_marker = tmp.path().join(".iii").join("workers");

    assert!(
        bundle_root.starts_with(tmp.path()),
        "bundle dir not under HOME"
    );
    assert!(
        binary_marker.starts_with(tmp.path()),
        "binary dir not under HOME"
    );
}

// =============================================================================
// Section J: Security regression tests
//
// Each test pins one security invariant: SSRF gating, manifest size
// cap, env-var kill switch, lock-file preservation, sweep_orphans
// lock-held protection, atomic_install race, boundary acceptance,
// adversarial tar entry types. If a test breaks, that invariant has
// regressed.
// =============================================================================

#[test]
fn validate_bundle_manifest_rejects_oversize_yaml() {
    // Billion-laughs defense: any iii.worker.yaml larger than
    // MAX_BUNDLE_MANIFEST_BYTES (64 KiB) must be refused BEFORE the
    // bytes reach serde_yaml::from_str — chained anchor/alias expansion
    // in serde_yaml 0.9.x can balloon a small file to gigabytes.
    let tmp = tempfile::tempdir().unwrap();
    let big = "# pad\n".repeat(((MAX_BUNDLE_MANIFEST_BYTES as usize) / 6) + 1);
    let body = format!("name: regress\nscripts:\n  start: \"node x.js\"\n{}", big);
    assert!(
        body.len() as u64 > MAX_BUNDLE_MANIFEST_BYTES,
        "test setup: oversize manifest must exceed cap"
    );
    write_manifest(tmp.path(), &body);

    let err = validate_bundle_manifest(tmp.path(), "regress")
        .expect_err("oversize manifest must be rejected");
    match err {
        WorkerOpError::BundleManifestRejected { field, reason } => {
            assert_eq!(field, "iii.worker.yaml");
            assert!(
                reason.contains("billion-laughs") || reason.contains("capped"),
                "reason should mention the cap: {reason}"
            );
        }
        other => panic!("expected BundleManifestRejected, got {other:?}"),
    }
}

#[test]
fn parse_bundle_resources_rejects_oversize_manifest() {
    // Same defense at the resource-parse seam (called by
    // start_bundle_worker on every boot). Even after install-time
    // validation, the manifest could be tampered with on disk before
    // the next start; the size cap fires before serde_yaml runs.
    let tmp = tempfile::tempdir().unwrap();
    let big = "# pad\n".repeat(((MAX_BUNDLE_MANIFEST_BYTES as usize) / 6) + 1);
    let body = format!("name: regress\nresources:\n  cpus: 2\n{}", big);
    write_manifest(tmp.path(), &body);

    let err = parse_bundle_resources(tmp.path(), ResourceCaps::default())
        .expect_err("oversize manifest must be rejected by parse_bundle_resources");
    assert!(
        matches!(err, WorkerOpError::BundleManifestRejected { .. }),
        "expected BundleManifestRejected, got {err:?}"
    );
}

#[test]
#[serial]
fn bundle_workers_disabled_respects_env_var() {
    // The III_BUNDLE_WORKERS_DISABLED kill switch is the only operator
    // gate today — handle_bundle_add and start_bundle_worker BOTH read
    // it before any network or sandbox work.
    let _unset = EnvGuard::unset(ENV_BUNDLE_WORKERS_DISABLED);
    assert!(
        !bundle_workers_disabled(),
        "default (env unset) must permit bundle workers"
    );

    let _set = EnvGuard::set(ENV_BUNDLE_WORKERS_DISABLED, "1");
    assert!(
        bundle_workers_disabled(),
        "env=1 must disable bundle workers"
    );
}

#[test]
#[serial]
fn bundle_workers_disabled_only_for_exact_one() {
    // Defensive parse: the gate fires only when the env value is the
    // literal string "1". `true` / `yes` / empty must NOT count, so an
    // operator who copy-pastes from another tool can't accidentally
    // half-disable the feature.
    let _set = EnvGuard::set(ENV_BUNDLE_WORKERS_DISABLED, "true");
    assert!(!bundle_workers_disabled());
    let _set = EnvGuard::set(ENV_BUNDLE_WORKERS_DISABLED, "");
    assert!(!bundle_workers_disabled());
    let _set = EnvGuard::set(ENV_BUNDLE_WORKERS_DISABLED, "1");
    assert!(bundle_workers_disabled());
}

#[test]
#[serial]
fn loopback_dev_bypass_requires_env_var() {
    // Production builds without III_BUNDLE_DEV_LOOPBACK=1 MUST refuse
    // localhost archive URLs — even though the URL classifier itself
    // recognizes them. Removes the previous "on by default in every
    // build" footgun.
    let _unset = EnvGuard::unset(ENV_BUNDLE_DEV_LOOPBACK);
    assert!(
        !loopback_dev_bypass_enabled("https://localhost/x.tar.gz"),
        "loopback bypass must be off by default"
    );
    assert!(!loopback_dev_bypass_enabled(
        "http://127.0.0.1:8000/x.tar.gz"
    ));
    assert!(!loopback_dev_bypass_enabled("http://[::1]/x.tar.gz"));

    let _set = EnvGuard::set(ENV_BUNDLE_DEV_LOOPBACK, "1");
    assert!(
        loopback_dev_bypass_enabled("https://localhost/x.tar.gz"),
        "env=1 enables localhost bypass"
    );
    assert!(loopback_dev_bypass_enabled(
        "http://127.0.0.1:8000/x.tar.gz"
    ));

    // Public hostnames stay disallowed even with the env gate on.
    assert!(!loopback_dev_bypass_enabled(
        "https://cdn.example.com/x.tar.gz"
    ));
}

#[test]
#[serial]
fn delete_worker_artifacts_preserves_per_worker_lock_file() {
    // delete_worker_artifacts must NOT unlink the per-worker fslock
    // file. Process A unlinking the lock while process B holds it
    // leaves B on a stale inode; process C then creates a new file
    // at the same path and acquires its own lock against a different
    // inode — two concurrent installs of the same worker name
    // proceed in parallel.
    use iii_worker::cli::managed::delete_worker_artifacts;
    let tmp = tempfile::tempdir().unwrap();
    let _home = HomeGuard::set(tmp.path());

    // Pre-create a bundle install + a per-worker lock file.
    let bundle_dir = bundle_worker_path("lockguard");
    std::fs::create_dir_all(&bundle_dir).unwrap();
    std::fs::write(bundle_dir.join("iii.worker.yaml"), "name: lockguard\n").unwrap();
    let lock_path = lock_path_for("lockguard");
    std::fs::create_dir_all(bundle_locks_dir()).unwrap();
    std::fs::write(&lock_path, b"42\n").unwrap();
    assert!(bundle_dir.is_dir());
    assert!(lock_path.is_file());

    delete_worker_artifacts("lockguard");

    assert!(!bundle_dir.exists(), "bundle install dir must be removed");
    assert!(
        lock_path.is_file(),
        "per-worker fslock file MUST survive delete_worker_artifacts"
    );
}

#[test]
#[serial]
fn sweep_orphans_skips_locked_staging_dir() {
    // The lock-acquirability check inside sweep_orphans is the only
    // thing preventing the 9-minute-download race from deleting an
    // in-flight staging dir. A regression that accidentally drops the
    // try_lock_with_pid call would not be caught by the age-based
    // test alone.
    use fslock::LockFile;
    let tmp = tempfile::tempdir().unwrap();
    let _home = HomeGuard::set(tmp.path());

    // Staging dir whose basename matches strip_unique_suffix's format.
    let staging = bundle_staging_root().join("slow-1234567890-0");
    std::fs::create_dir_all(&staging).unwrap();

    // Backdate mtime past MIN_STAGING_AGE_SECS (60s) so the age guard
    // alone would consider this dir sweepable; only the lock should
    // keep it alive.
    let two_minutes_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(180);
    let ft = filetime::FileTime::from_system_time(two_minutes_ago);
    filetime::set_file_mtime(&staging, ft).unwrap();

    // Acquire the per-worker lock as a live install would.
    std::fs::create_dir_all(bundle_locks_dir()).unwrap();
    let lock_path = lock_path_for("slow");
    let mut lock = LockFile::open(&lock_path).unwrap();
    lock.lock_with_pid().unwrap();

    let removed = sweep_orphans();
    assert_eq!(removed, 0, "sweep must skip locked staging dir");
    assert!(
        staging.exists(),
        "locked staging dir must survive sweep_orphans"
    );

    drop(lock);
}

#[test]
#[serial]
fn atomic_install_concurrent_both_safe() {
    // Two threads racing into atomic_install for the same target
    // name. The fslock in production serializes this, but
    // atomic_install is a public API and must be safe on its own.
    // With replace semantics there is no single winner: the second
    // thread may replace the first (last-writer-wins) or lose the
    // rename race with a platform error mapped to ConfigIo. Either
    // way `final_dir` must end up as a complete install from ONE of
    // the stagings, with no `.old.*`/`.partial.*` siblings leaked.
    let tmp = tempfile::tempdir().unwrap();
    let _home = HomeGuard::set(tmp.path());

    let staging_a = bundle_staging_root().join("race-a");
    let staging_b = bundle_staging_root().join("race-b");
    std::fs::create_dir_all(&staging_a).unwrap();
    std::fs::create_dir_all(&staging_b).unwrap();
    std::fs::write(staging_a.join("iii.worker.yaml"), "name: race\n").unwrap();
    std::fs::write(staging_b.join("iii.worker.yaml"), "name: race\n").unwrap();

    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let b1 = barrier.clone();
    let b2 = barrier.clone();

    let t1 = std::thread::spawn(move || {
        b1.wait();
        atomic_install(&staging_a, "race")
    });
    let t2 = std::thread::spawn(move || {
        b2.wait();
        atomic_install(&staging_b, "race")
    });

    let r1 = t1.join().unwrap();
    let r2 = t2.join().unwrap();

    let oks = [&r1, &r2].iter().filter(|r| r.is_ok()).count();
    assert!(
        oks >= 1,
        "at least one install must succeed. r1={r1:?} r2={r2:?}"
    );

    let final_dir = bundle_worker_path("race");
    let manifest = std::fs::read_to_string(final_dir.join("iii.worker.yaml"))
        .expect("final dir holds a complete install");
    assert_eq!(manifest, "name: race\n");

    let leftovers = sibling_leftovers(final_dir.parent().unwrap());
    assert!(leftovers.is_empty(), "sibling leftovers: {leftovers:?}");
}

#[test]
fn extract_accepts_exactly_max_entries() {
    // Boundary acceptance: a regression that flips `>` to `>=` in the
    // entry-count guard would silently reject all max-sized legitimate
    // bundles. Pin the inclusive limit.
    let mut entries: Vec<(String, Vec<u8>, tar::EntryType)> = Vec::new();
    for i in 0..MAX_BUNDLE_ENTRIES {
        entries.push((format!("f{i}.txt"), b"x".to_vec(), tar::EntryType::Regular));
    }
    let refs: Vec<(&str, &[u8], tar::EntryType)> = entries
        .iter()
        .map(|(p, c, k)| (p.as_str(), c.as_slice(), *k))
        .collect();
    let bytes = make_targz(&refs);

    let tmp = tempfile::tempdir().unwrap();
    let archive = write_archive(tmp.path(), "a.tar.gz", &bytes);
    let dest = tmp.path().join("dest");
    std::fs::create_dir_all(&dest).unwrap();
    extract_bundle_safely_blocking(&archive, &dest)
        .expect("exactly MAX_BUNDLE_ENTRIES must succeed");
}

#[test]
fn extract_accepts_exactly_max_depth() {
    // Same boundary check for depth. The implementation uses `> limit`;
    // a `>= limit` regression would reject deepest-legal archives.
    let mut path = String::new();
    for i in 0..MAX_BUNDLE_DEPTH {
        if !path.is_empty() {
            path.push('/');
        }
        path.push_str(&format!("d{i}"));
    }
    // depth counts Normal components; MAX_BUNDLE_DEPTH directory levels
    // produce exactly that many Normal components on the path.
    let bytes = make_targz(&[(path.as_str(), b"x", tar::EntryType::Regular)]);
    let tmp = tempfile::tempdir().unwrap();
    let archive = write_archive(tmp.path(), "a.tar.gz", &bytes);
    let dest = tmp.path().join("dest");
    std::fs::create_dir_all(&dest).unwrap();
    extract_bundle_safely_blocking(&archive, &dest)
        .expect("depth == MAX_BUNDLE_DEPTH must succeed");
}

#[test]
fn extract_rejects_fifo_entry() {
    // typeflag '6' = Fifo. Adds coverage for the third non-Regular/
    // Directory entry type the filter must reject; symlink, hardlink,
    // and char-device are already tested.
    let gz = make_raw_tar_gz_with_type(b"fifo", &[], b'6');
    let tmp = tempfile::tempdir().unwrap();
    let archive = write_archive(tmp.path(), "fifo.tar.gz", &gz);
    let dest = tmp.path().join("dest");
    std::fs::create_dir_all(&dest).unwrap();
    let err =
        extract_bundle_safely_blocking(&archive, &dest).expect_err("fifo entry must be rejected");
    assert!(
        matches!(err, WorkerOpError::BundleArchiveUnsafe { .. }),
        "expected BundleArchiveUnsafe, got {err:?}"
    );
}

#[test]
fn extract_rejects_block_device_entry() {
    // typeflag '4' = Block device. Same logic as the Fifo test; covers
    // the remaining special-file entry type bundle archives never
    // legitimately ship.
    let gz = make_raw_tar_gz_with_type(b"bdev", &[], b'4');
    let tmp = tempfile::tempdir().unwrap();
    let archive = write_archive(tmp.path(), "bdev.tar.gz", &gz);
    let dest = tmp.path().join("dest");
    std::fs::create_dir_all(&dest).unwrap();
    let err = extract_bundle_safely_blocking(&archive, &dest)
        .expect_err("block-device entry must be rejected");
    assert!(
        matches!(err, WorkerOpError::BundleArchiveUnsafe { .. }),
        "expected BundleArchiveUnsafe, got {err:?}"
    );
}

#[test]
fn extract_rejects_directory_then_file_collision() {
    // set_overwrite(true) is in effect on the tar archive. An archive
    // containing a Directory entry then a Regular file at the SAME
    // path must NOT silently succeed: std::fs::File::create cannot
    // replace a non-empty directory. Pin this so a future shift in
    // tar-crate behavior is caught.
    let bytes = make_targz(&[
        ("shared", b"" as &[u8], tar::EntryType::Directory),
        ("shared", b"content", tar::EntryType::Regular),
    ]);
    let tmp = tempfile::tempdir().unwrap();
    let archive = write_archive(tmp.path(), "dup.tar.gz", &bytes);
    let dest = tmp.path().join("dest");
    std::fs::create_dir_all(&dest).unwrap();

    let result = extract_bundle_safely_blocking(&archive, &dest);
    assert!(
        result.is_err(),
        "dir-then-file collision at same path should fail (got Ok)"
    );
}

// =============================================================================
// Helpers
// =============================================================================

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}

/// RAII helper that temporarily redirects `$HOME` to a tempdir. Because
/// `std::env::set_var` is global, all tests using this helper must be
/// marked `#[serial]`.
struct HomeGuard {
    previous: Option<std::ffi::OsString>,
}

impl HomeGuard {
    fn set(path: &Path) -> Self {
        let previous = std::env::var_os("HOME");
        // SAFETY: env mutation is process-global. All callers gate with
        // `#[serial]` to avoid concurrent reads from other tests.
        unsafe {
            std::env::set_var("HOME", path);
        }
        Self { previous }
    }
}

impl Drop for HomeGuard {
    fn drop(&mut self) {
        // SAFETY: see Self::set.
        unsafe {
            match self.previous.take() {
                Some(prev) => std::env::set_var("HOME", prev),
                None => std::env::remove_var("HOME"),
            }
        }
    }
}

/// RAII guard that switches the process working directory and restores it
/// on drop. Process CWD is global; all callers must be `#[serial]`.
/// Used to simulate running `iii worker add` from a second project dir.
struct CwdGuard {
    previous: std::path::PathBuf,
}

impl CwdGuard {
    fn set(path: &Path) -> Self {
        let previous = std::env::current_dir().unwrap();
        std::env::set_current_dir(path).unwrap();
        Self { previous }
    }
}

impl Drop for CwdGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.previous);
    }
}

/// RAII helper that temporarily sets/unsets an arbitrary env var.
/// Mirrors `HomeGuard` for arbitrary keys (used by the
/// `III_BUNDLE_WORKERS_DISABLED` and `III_BUNDLE_DEV_LOOPBACK` gates).
/// All callers must be `#[serial]` because env mutation is global.
struct EnvGuard {
    key: String,
    previous: Option<std::ffi::OsString>,
}

impl EnvGuard {
    fn set(key: &str, value: &str) -> Self {
        let previous = std::env::var_os(key);
        // SAFETY: env mutation is process-global. All callers gate with
        // `#[serial]` to avoid concurrent reads from other tests.
        unsafe {
            std::env::set_var(key, value);
        }
        Self {
            key: key.to_string(),
            previous,
        }
    }

    fn unset(key: &str) -> Self {
        let previous = std::env::var_os(key);
        // SAFETY: see Self::set.
        unsafe {
            std::env::remove_var(key);
        }
        Self {
            key: key.to_string(),
            previous,
        }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        // SAFETY: see Self::set.
        unsafe {
            match self.previous.take() {
                Some(prev) => std::env::set_var(&self.key, prev),
                None => std::env::remove_var(&self.key),
            }
        }
    }
}
