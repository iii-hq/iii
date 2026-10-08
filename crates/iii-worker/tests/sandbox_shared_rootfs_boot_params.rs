//! The shared image cache must never be a sandbox's root (daemon side).
//!
//! `handle_create` used to hand the launcher the shared, cached image
//! directory itself as the VM root
//! (`BootParams.rootfs = rootfs_cache::resolve_cached(..)`), and
//! `IiiWorkerLauncher` passed it verbatim as `__vm-boot --rootfs`, with no
//! `--rootfs-mode overlay` / read-only lower. Two sandboxes (from two
//! different callers) therefore shared one writable root.
//!
//! Required: each sandbox boots from its own view (a read-only lower plus a
//! per-sandbox upper), so two sandboxes never share a writable root and the
//! cached image is not the guest-visible root.

use iii_worker::sandbox_daemon::config::SandboxConfig;
use iii_worker::sandbox_daemon::{
    SandboxError,
    create::{BootHandle, BootParams, CreateRequest, VmLauncher, handle_create},
    exec::EnvShape,
    registry::SandboxRegistry,
};
use serial_test::serial;
use std::path::PathBuf;
use std::sync::Mutex;

struct RecordingLauncher {
    seen: Mutex<Vec<PathBuf>>,
}
#[async_trait::async_trait]
impl VmLauncher for RecordingLauncher {
    async fn boot(&self, p: &BootParams) -> Result<BootHandle, SandboxError> {
        self.seen.lock().unwrap().push(p.rootfs.clone());
        Ok(BootHandle {
            vm_pid: 999_999,
            lifeline: None,
        })
    }
}

#[allow(dead_code)]
struct HomeGuard(tempfile::TempDir, Option<std::ffi::OsString>);
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
fn fake_home_with_cached_image(image: &str) -> (HomeGuard, PathBuf) {
    let td = tempfile::tempdir().unwrap();
    let rootfs = td.path().join(".iii/managed").join(image).join("rootfs");
    std::fs::create_dir_all(rootfs.join("bin")).unwrap();
    let orig = std::env::var_os("HOME");
    // SAFETY: #[serial]; no other thread in this binary touches env.
    unsafe { std::env::set_var("HOME", td.path()) };
    (HomeGuard(td, orig), rootfs)
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
async fn two_sandboxes_must_not_share_the_cached_image_as_writable_root() {
    let (_g, cached) = fake_home_with_cached_image("python");
    let cfg = SandboxConfig {
        auto_install: false,
        image_allowlist: vec!["python".into()],
        ..Default::default()
    };
    let reg = SandboxRegistry::new();
    let l = RecordingLauncher {
        seen: Mutex::new(vec![]),
    };
    let a = handle_create(req(), &cfg, &reg, &l, |_| {}).await.unwrap();
    let b = handle_create(req(), &cfg, &reg, &l, |_| {}).await.unwrap();
    let seen = l.seen.lock().unwrap().clone();
    eprintln!("sandbox A {} rootfs = {}", a.sandbox_id, seen[0].display());
    eprintln!("sandbox B {} rootfs = {}", b.sandbox_id, seen[1].display());
    eprintln!("shared cached image  = {}", cached.display());

    // cleanup overlay dirs created under /tmp/iii-sandbox
    for s in [&a.sandbox_id, &b.sandbox_id] {
        let _ = std::fs::remove_dir_all(PathBuf::from("/tmp/iii-sandbox").join(s));
    }

    assert_ne!(
        seen[0], cached,
        "the VM root handed to __vm-boot is the shared image cache itself"
    );
    assert_ne!(
        seen[0], seen[1],
        "two sandboxes booted with the same writable root directory"
    );
}
