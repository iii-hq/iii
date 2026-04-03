//! Integration tests for iii-filesystem.
//!
//! These tests verify that the filesystem components work together:
//! PassthroughFs construction via builder, and basic construction
//! on a real temporary directory.

use std::time::Duration;

use iii_filesystem::{CachePolicy, PassthroughConfig, PassthroughFs};

/// Test 4: Filesystem mount -- builder creates a functional PassthroughFs
/// pointing at a real host directory.
#[test]
fn builder_creates_functional_passthrough_fs() {
    let dir = tempfile::tempdir().unwrap();

    // Create a file in the directory
    std::fs::write(dir.path().join("test.txt"), "hello world").unwrap();

    let result = PassthroughFs::builder()
        .root_dir(dir.path())
        .entry_timeout(Duration::from_secs(10))
        .attr_timeout(Duration::from_secs(10))
        .cache_policy(CachePolicy::Auto)
        .build();

    assert!(
        result.is_ok(),
        "Builder should create a valid PassthroughFs"
    );
}

/// Test 4 (continued): PassthroughFs::new with explicit config.
#[test]
fn new_creates_passthrough_fs_with_config() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = PassthroughConfig {
        root_dir: dir.path().to_path_buf(),
        entry_timeout: Duration::from_secs(1),
        attr_timeout: Duration::from_secs(2),
        cache_policy: CachePolicy::Never,
        writeback: true,
    };

    let result = PassthroughFs::new(cfg);
    assert!(
        result.is_ok(),
        "PassthroughFs::new should succeed with valid config"
    );
}

/// Test 4 (continued): All cache policies are constructible.
#[test]
fn all_cache_policies_construct_successfully() {
    let dir = tempfile::tempdir().unwrap();

    for policy in [CachePolicy::Never, CachePolicy::Auto, CachePolicy::Always] {
        let result = PassthroughFs::builder()
            .root_dir(dir.path())
            .cache_policy(policy)
            .build();

        assert!(result.is_ok(), "Cache policy {:?} should work", policy);
    }
}

/// Builder rejects nonexistent root directory.
#[test]
fn builder_rejects_nonexistent_root() {
    let result = PassthroughFs::builder()
        .root_dir("/nonexistent_path_xyz_12345")
        .build();
    assert!(result.is_err());
}

/// Builder rejects missing root_dir (default empty path).
#[test]
fn builder_rejects_missing_root_dir() {
    let result = PassthroughFs::builder().build();
    assert!(result.is_err());
}

/// PassthroughFs::new rejects nonexistent root directory.
#[test]
fn new_rejects_nonexistent_root() {
    let cfg = PassthroughConfig {
        root_dir: "/nonexistent_dir_abc_67890".into(),
        ..Default::default()
    };
    let result = PassthroughFs::new(cfg);
    assert!(result.is_err());
}

/// Builder with writeback enabled succeeds.
#[test]
fn builder_with_writeback_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let result = PassthroughFs::builder()
        .root_dir(dir.path())
        .writeback(true)
        .build();
    assert!(result.is_ok());
}

/// PassthroughConfig has correct defaults.
#[test]
fn passthrough_config_defaults() {
    let cfg = PassthroughConfig::default();
    assert_eq!(cfg.entry_timeout, Duration::from_secs(5));
    assert_eq!(cfg.attr_timeout, Duration::from_secs(5));
    assert_eq!(cfg.cache_policy, CachePolicy::Auto);
    assert!(!cfg.writeback);
}

/// Builder with all options produces a valid filesystem.
#[test]
fn builder_full_options() {
    let dir = tempfile::tempdir().unwrap();
    let result = PassthroughFs::builder()
        .root_dir(dir.path())
        .entry_timeout(Duration::from_millis(500))
        .attr_timeout(Duration::from_millis(1000))
        .cache_policy(CachePolicy::Always)
        .writeback(true)
        .build();
    assert!(result.is_ok());
}
