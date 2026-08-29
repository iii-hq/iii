//! Compose-facing local VM preparation.

use std::path::Path;

use iii_worker::cli::local_worker::{VmOverride, local_vm_command};

fn worker_dir(tmp: &Path, manifest: &str) -> std::path::PathBuf {
    let dir = tmp.join("worker");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("iii.worker.yaml"), manifest).unwrap();
    dir
}

fn over(tmp: &Path) -> VmOverride<'static> {
    VmOverride {
        state_dir: tmp.join("vm"),
        engine_url: "ws://localhost:49134",
        extra_env: std::collections::HashMap::new(),
        config_dir: None,
    }
}

#[tokio::test]
async fn a_missing_base_image_is_refused_before_vm_setup() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = worker_dir(
        tmp.path(),
        "name: probe\nscripts:\n  start: python src/main.py\n",
    );

    let error = local_vm_command("probe", &dir, None, over(tmp.path()))
        .await
        .err()
        .expect("compose local VM preparation requires base_image");
    assert!(error.contains("runtime.base_image"), "{error}");
}

#[tokio::test]
async fn an_implausible_base_image_is_refused_before_vm_setup() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = worker_dir(
        tmp.path(),
        "name: probe\nruntime:\n  base_image: 'bad image!'\nscripts:\n  start: python src/main.py\n",
    );

    let error = local_vm_command("probe", &dir, None, over(tmp.path()))
        .await
        .err()
        .expect("an invalid OCI reference must be rejected");
    assert!(error.contains("plausible OCI image"), "{error}");
}

/// The success path prepares a rootfs and therefore may pull the declared OCI
/// image. It also proves that a compose container key may replace the manifest
/// name and start command for a local project.
#[tokio::test]
#[ignore = "pulls an OCI base image"]
async fn a_local_worker_builds_a_boot_command_with_the_compose_run_override() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = worker_dir(
        tmp.path(),
        "name: manifest-name\nruntime:\n  base_image: docker.io/iiidev/python:latest\nscripts:\n  install: pip install -e .\n",
    );

    let built = local_vm_command(
        "compose-name",
        &dir,
        Some("python src/dev.py"),
        over(tmp.path()),
    )
    .await
    .expect("a valid local worker should build");

    let rendered = format!("{:?}", built.command.as_std());
    assert!(rendered.contains("__vm-boot"), "{rendered}");

    let script = std::fs::read_to_string(tmp.path().join("vm/runtime/dev-run.sh"))
        .or_else(|_| std::fs::read_to_string(tmp.path().join("vm/opt/iii/dev-run.sh")))
        .expect("VM preparation should write its run script");
    assert!(script.contains("python src/dev.py"), "{script}");
}
