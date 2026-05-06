// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Shared rootfs cloning utility.

/// Clone a rootfs directory using APFS clonefile (macOS) or reflink (Linux).
pub fn clone_rootfs(base: &std::path::Path, dest: &std::path::Path) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {}", e))?;
    }
    let status = std::process::Command::new("cp")
        .args(if cfg!(target_os = "macos") {
            vec!["-c", "-a"]
        } else {
            vec!["--reflink=auto", "-a"]
        })
        .arg(base.as_os_str())
        .arg(dest.as_os_str())
        .status()
        .map_err(|e| format!("cp: {}", e))?;
    if !status.success() {
        return Err(format!("cp exited with {}", status));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clone_rootfs_copies_directory() {
        let src = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(src.path().join("bin")).unwrap();
        std::fs::write(src.path().join("bin/hello"), "world").unwrap();
        std::fs::write(src.path().join("root.txt"), "data").unwrap();

        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("cloned");

        clone_rootfs(src.path(), &dest).unwrap();

        assert!(dest.join("bin/hello").exists());
        assert_eq!(
            std::fs::read_to_string(dest.join("bin/hello")).unwrap(),
            "world"
        );
        assert_eq!(
            std::fs::read_to_string(dest.join("root.txt")).unwrap(),
            "data"
        );
    }

    #[test]
    fn clone_rootfs_creates_parent_dirs() {
        let src = tempfile::tempdir().unwrap();
        std::fs::write(src.path().join("file.txt"), "content").unwrap();

        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("deep").join("nested").join("clone");

        clone_rootfs(src.path(), &dest).unwrap();

        assert!(dest.join("file.txt").exists());
    }

    #[test]
    fn clone_rootfs_fails_on_missing_source() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("does_not_exist");
        let dest = tmp.path().join("dest");

        let result = clone_rootfs(&missing, &dest);
        assert!(result.is_err());
    }
}
