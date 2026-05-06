// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use std::path::Path;

const CONFIG_TEMPLATE: &str = include_str!("templates/config.yaml.template");
const LOCK_TEMPLATE: &str = include_str!("templates/iii.lock.template");
const GITIGNORE_TEMPLATE: &str = include_str!("templates/gitignore.template");

pub fn write_scaffold(project_root: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(project_root.join("data"))?;
    write_if_absent(&project_root.join("config.yaml"), CONFIG_TEMPLATE)?;
    write_if_absent(&project_root.join("iii.lock"), LOCK_TEMPLATE)?;
    write_if_absent(&project_root.join(".gitignore"), GITIGNORE_TEMPLATE)?;
    Ok(())
}

fn write_if_absent(path: &Path, contents: &str) -> std::io::Result<()> {
    if path.exists() {
        return Ok(());
    }
    std::fs::write(path, contents)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn writes_all_scaffold_files() {
        let dir = tempdir().unwrap();
        write_scaffold(dir.path()).unwrap();
        assert!(dir.path().join("config.yaml").exists());
        assert!(dir.path().join("iii.lock").exists());
        assert!(dir.path().join(".gitignore").exists());
        assert!(dir.path().join("data").is_dir());
    }

    #[test]
    fn gitignore_includes_dot_iii() {
        let dir = tempdir().unwrap();
        write_scaffold(dir.path()).unwrap();
        let gi = std::fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        assert!(gi.contains(".iii/"));
        assert!(gi.contains("node_modules/"));
        assert!(gi.contains("target/"));
        assert!(gi.contains("__pycache__"));
    }

    #[test]
    fn does_not_overwrite_existing_files() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("config.yaml"), "existing: true\n").unwrap();
        write_scaffold(dir.path()).unwrap();
        let content = std::fs::read_to_string(dir.path().join("config.yaml")).unwrap();
        assert_eq!(content, "existing: true\n");
    }
}
