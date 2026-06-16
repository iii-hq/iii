use scaffolder_core::cli::{check_directory_state, resolve_root};
use std::fs;
use tempfile::tempdir;

#[test]
fn resolve_root_uses_cwd_when_none() {
    let cwd = std::env::current_dir().unwrap();
    let resolved = resolve_root(None).unwrap();
    assert_eq!(resolved, cwd);
}

#[test]
fn resolve_root_rejects_empty_string() {
    assert!(resolve_root(Some("")).is_err());
}

#[test]
fn resolve_root_accepts_explicit_path() {
    let resolved = resolve_root(Some("/tmp/foo")).unwrap();
    assert_eq!(resolved, std::path::PathBuf::from("/tmp/foo"));
}

#[test]
fn check_directory_state_passes_for_empty_dir() {
    let dir = tempdir().unwrap();
    assert!(check_directory_state(dir.path(), false, "worker.ini").is_ok());
}

#[test]
fn check_directory_state_allows_dotfiles() {
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join(".git")).unwrap();
    assert!(check_directory_state(dir.path(), false, "worker.ini").is_ok());
}

#[test]
fn check_directory_state_rejects_data_dir_without_flag() {
    // `data/` is engine-owned runtime state; scaffolding next to it would
    // orphan that engine's project. It must NOT be exempt like dotfiles.
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("data")).unwrap();
    assert!(check_directory_state(dir.path(), false, "worker.ini").is_err());
}

#[test]
fn check_directory_state_rejects_user_files_without_flag() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("user.txt"), "x").unwrap();
    assert!(check_directory_state(dir.path(), false, "worker.ini").is_err());
}

#[test]
fn check_directory_state_passes_when_marker_file_present() {
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join(".iii")).unwrap();
    fs::write(
        dir.path().join(".iii").join("worker.ini"),
        "[worker]\nworker_id=x\n",
    )
    .unwrap();
    fs::write(dir.path().join("user.txt"), "x").unwrap();
    assert!(check_directory_state(dir.path(), false, "worker.ini").is_ok());
}
