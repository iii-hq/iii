//! Soname symlink creation for libkrunfw.

use std::path::Path;

use super::constants::{LIBKRUNFW_ABI, LIBKRUNFW_VERSION};

/// Create soname symlinks for libkrunfw in the given directory.
#[cfg(unix)]
pub fn create_libkrunfw_symlinks(lib_dir: &Path) {
    let symlinks = libkrunfw_symlink_pairs();

    for (link_name, target) in &symlinks {
        let link_path = lib_dir.join(link_name);

        if link_path.exists() || link_path.is_symlink() {
            let _ = std::fs::remove_file(&link_path);
        }

        if let Err(e) = std::os::unix::fs::symlink(target, &link_path) {
            tracing::warn!(
                link = %link_path.display(),
                target = %target,
                error = %e,
                "failed to create libkrunfw symlink"
            );
        }
    }
}

fn libkrunfw_symlink_pairs() -> Vec<(String, String)> {
    if cfg!(target_os = "macos") {
        vec![(
            "libkrunfw.dylib".to_string(),
            format!("libkrunfw.{LIBKRUNFW_ABI}.dylib"),
        )]
    } else {
        let soname = format!("libkrunfw.so.{LIBKRUNFW_ABI}");
        let versioned = format!("libkrunfw.so.{LIBKRUNFW_VERSION}");
        vec![
            (soname.clone(), versioned),
            ("libkrunfw.so".to_string(), soname),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_symlink_pairs() {
        let pairs = libkrunfw_symlink_pairs();
        if cfg!(target_os = "macos") {
            assert_eq!(pairs.len(), 1);
            assert_eq!(pairs[0].0, "libkrunfw.dylib");
            assert_eq!(pairs[0].1, "libkrunfw.5.dylib");
        } else {
            assert_eq!(pairs.len(), 2);
            assert_eq!(pairs[0].0, "libkrunfw.so.5");
            assert_eq!(pairs[0].1, "libkrunfw.so.5.2.1");
            assert_eq!(pairs[1].0, "libkrunfw.so");
            assert_eq!(pairs[1].1, "libkrunfw.so.5");
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_create_symlinks_idempotent() {
        let tmp = TempDir::new().unwrap();
        let filename = super::super::constants::libkrunfw_filename();
        std::fs::write(tmp.path().join(&filename), b"firmware").unwrap();

        create_libkrunfw_symlinks(tmp.path());
        create_libkrunfw_symlinks(tmp.path());

        let pairs = libkrunfw_symlink_pairs();
        for (link_name, target) in &pairs {
            let link_path = tmp.path().join(link_name);
            assert!(link_path.is_symlink());
            assert_eq!(
                std::fs::read_link(&link_path).unwrap().to_str().unwrap(),
                target
            );
        }
    }
}
