//! Daemon-side checks on what a sandbox boots from, complementing
//! `sandbox_shared_rootfs_boot_params`.
//!
//! - That test exercises the legacy fallback `~/.iii/managed/<image>/rootfs/`.
//!   This one covers the primary cache, `~/.iii/cache/<slug>/`
//!   (`rootfs_cache::canonical_path`).
//! - It records the full `BootParams`: the cache must arrive only as
//!   `base_rootfs` (the erofs lower source), never as the guest root, and the
//!   guest root must be a per-sandbox trampoline holding nothing but the
//!   staging dirs iii-init needs.
//! - A failed launcher `preflight` must stop `handle_create` before the image
//!   is resolved or pulled.
//!
//! No VM: the launchers only record what they are handed.

use iii_worker::cli::rootfs_cache;
use iii_worker::sandbox_daemon::config::SandboxConfig;
use iii_worker::sandbox_daemon::overlay::TRAMPOLINE_STAGING_DIRS;
use iii_worker::sandbox_daemon::{
    SandboxError, catalog,
    create::{BootHandle, BootParams, CreateRequest, VmLauncher, handle_create},
    exec::EnvShape,
    registry::SandboxRegistry,
};
use serial_test::serial;
use std::path::PathBuf;
use std::sync::Mutex;

/// What one `boot` call was handed, observed at boot time.
struct Seen {
    rootfs: PathBuf,
    base_rootfs: PathBuf,
    /// Entries of `rootfs` at boot time, sorted.
    trampoline_entries: Vec<String>,
}

struct RecordingLauncher {
    seen: Mutex<Vec<Seen>>,
}
#[async_trait::async_trait]
impl VmLauncher for RecordingLauncher {
    async fn boot(&self, p: &BootParams) -> Result<BootHandle, SandboxError> {
        let mut trampoline_entries: Vec<String> = std::fs::read_dir(&p.rootfs)
            .map(|rd| {
                rd.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        trampoline_entries.sort();
        self.seen.lock().unwrap().push(Seen {
            rootfs: p.rootfs.clone(),
            base_rootfs: p.base_rootfs.clone(),
            trampoline_entries,
        });
        Ok(BootHandle {
            vm_pid: 999_999,
            lifeline: None,
        })
    }
}

/// Launcher whose host cannot boot an isolated root (no embedded iii-init).
struct RefusingLauncher;
#[async_trait::async_trait]
impl VmLauncher for RefusingLauncher {
    fn preflight(&self) -> Result<(), SandboxError> {
        Err(SandboxError::BootFailed("no embedded iii-init".into()))
    }
    async fn boot(&self, _p: &BootParams) -> Result<BootHandle, SandboxError> {
        panic!("boot must not run after a failed preflight");
    }
}

struct HomeGuard(
    #[allow(dead_code)] tempfile::TempDir,
    Option<std::ffi::OsString>,
);
impl Drop for HomeGuard {
    fn drop(&mut self) {
        unsafe {
            match &self.1 {
                Some(v) => std::env::set_var("HOME", v),
                None => std::env::remove_var("HOME"),
            }
        }
    }
}

/// Point HOME at a fresh temp dir for the duration of the test.
fn isolated_home() -> HomeGuard {
    let td = tempfile::tempdir().unwrap();
    let orig = std::env::var_os("HOME");
    // SAFETY: #[serial]; no other thread in this binary touches the env.
    unsafe { std::env::set_var("HOME", td.path()) };
    HomeGuard(td, orig)
}

fn cfg() -> SandboxConfig {
    SandboxConfig {
        auto_install: false,
        image_allowlist: vec!["python".into()],
        ..Default::default()
    }
}

fn req() -> CreateRequest {
    CreateRequest {
        image: "python".into(),
        cpus: None,
        memory_mb: None,
        name: None,
        network: None,
        idle_timeout_secs: None,
        env: EnvShape::default(),
    }
}

#[tokio::test]
#[serial]
async fn canonical_cache_is_only_the_lower_never_the_guest_root() {
    let _home = isolated_home();
    let cfg = cfg();
    let oci_ref = catalog::resolve_image("python", &cfg.custom_images).unwrap();
    let canonical = rootfs_cache::canonical_path(&oci_ref);
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    assert!(canonical.starts_with(&home), "isolated HOME not honoured");
    std::fs::create_dir_all(canonical.join("bin")).unwrap();
    std::fs::write(canonical.join("bin/marker"), b"original").unwrap();

    let reg = SandboxRegistry::new();
    let l = RecordingLauncher {
        seen: Mutex::new(vec![]),
    };
    let a = handle_create(req(), &cfg, &reg, &l, |_| {}).await.unwrap();
    let b = handle_create(req(), &cfg, &reg, &l, |_| {}).await.unwrap();
    let seen = std::mem::take(&mut *l.seen.lock().unwrap());
    eprintln!("oci_ref         = {oci_ref}");
    eprintln!("canonical cache = {}", canonical.display());
    for (id, s) in [(&a.sandbox_id, &seen[0]), (&b.sandbox_id, &seen[1])] {
        eprintln!(
            "sandbox {id}: rootfs = {} base_rootfs = {} trampoline = {:?}",
            s.rootfs.display(),
            s.base_rootfs.display(),
            s.trampoline_entries
        );
    }

    for s in [&a.sandbox_id, &b.sandbox_id] {
        let _ = std::fs::remove_dir_all(PathBuf::from("/tmp/iii-sandbox").join(s));
    }

    let mut staging: Vec<String> = TRAMPOLINE_STAGING_DIRS
        .iter()
        .map(|d| d.to_string())
        .collect();
    staging.sort();
    for s in &seen {
        assert_eq!(
            s.base_rootfs, canonical,
            "the cache must arrive as the lower source"
        );
        assert_ne!(
            s.rootfs, canonical,
            "the VM root handed to __vm-boot is the shared canonical cache itself"
        );
        assert!(
            !s.rootfs.starts_with(&canonical),
            "the VM root must live outside the cache"
        );
        assert_eq!(
            s.trampoline_entries, staging,
            "the trampoline must hold only iii-init's staging dirs"
        );
    }
    assert_ne!(
        seen[0].rootfs, seen[1].rootfs,
        "two sandboxes with the same writable root"
    );
    assert_eq!(
        std::fs::read(canonical.join("bin/marker")).unwrap(),
        b"original",
        "create must not write into the shared cache"
    );
}

#[tokio::test]
#[serial]
async fn failed_preflight_stops_create_before_the_image_is_resolved() {
    // No cached image and no auto-install: resolving the image first would
    // fail with RootfsMissing (S101); the preflight must win with S300.
    let _home = isolated_home();
    let reg = SandboxRegistry::new();
    let err = handle_create(req(), &cfg(), &reg, &RefusingLauncher, |_| {})
        .await
        .expect_err("create must fail when preflight fails");
    assert_eq!(err.code().as_str(), "S300");
    assert_eq!(reg.count().await, 0);
}
