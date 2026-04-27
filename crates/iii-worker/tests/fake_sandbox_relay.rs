//! Integration tests for the sandbox subsystem. Uses fake trait impls for
//! VmLauncher/ShellRunner/VmStopper so no real libkrun boot is required.

use iii_worker::sandbox_daemon::config::SandboxConfig;
use iii_worker::sandbox_daemon::{
    SandboxError,
    create::{BootHandle, BootParams, CreateRequest, VmLauncher, handle_create},
    exec::{ExecRequest, ExecResponse, ShellRunner, handle_exec},
    registry::SandboxRegistry,
    stop::{StopRequest, VmStopper, handle_stop},
};
use serial_test::serial;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

struct TestHarness {
    launcher: Arc<FakeLauncher>,
    runner: Arc<FakeRunner>,
    stopper: Arc<FakeStopper>,
    registry: SandboxRegistry,
}

struct FakeLauncher {
    boot_count: AtomicU32,
}
#[async_trait::async_trait]
impl VmLauncher for FakeLauncher {
    async fn boot(&self, _p: &BootParams) -> Result<BootHandle, SandboxError> {
        let n = self.boot_count.fetch_add(1, Ordering::SeqCst) + 1;
        Ok(BootHandle { vm_pid: 10_000 + n })
    }
}

struct FakeRunner {
    stdout: String,
    exit: i32,
}
#[async_trait::async_trait]
impl ShellRunner for FakeRunner {
    async fn run(
        &self,
        _sock: std::path::PathBuf,
        _r: &ExecRequest,
    ) -> Result<ExecResponse, SandboxError> {
        Ok(ExecResponse {
            stdout: self.stdout.clone(),
            stderr: String::new(),
            exit_code: Some(self.exit),
            timed_out: false,
            duration_ms: 1,
            success: self.exit == 0,
        })
    }
}

struct FakeStopper {
    stops: AtomicU32,
}
#[async_trait::async_trait]
impl VmStopper for FakeStopper {
    async fn stop(&self, _pid: u32) -> Result<(), SandboxError> {
        self.stops.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

fn harness(runner_stdout: &str, runner_exit: i32) -> TestHarness {
    TestHarness {
        launcher: Arc::new(FakeLauncher {
            boot_count: AtomicU32::new(0),
        }),
        runner: Arc::new(FakeRunner {
            stdout: runner_stdout.into(),
            exit: runner_exit,
        }),
        stopper: Arc::new(FakeStopper {
            stops: AtomicU32::new(0),
        }),
        registry: SandboxRegistry::new(),
    }
}

fn cfg_all_presets_allowed() -> SandboxConfig {
    SandboxConfig {
        auto_install: false,
        image_allowlist: vec!["python".into(), "node".into()],
        ..Default::default()
    }
}

#[tokio::test]
#[serial]
async fn end_to_end_create_exec_stop() {
    let _guard = ensure_fake_rootfs("python");
    let h = harness("hi\n", 0);
    let cfg = cfg_all_presets_allowed();
    let req = CreateRequest {
        image: "python".into(),
        cpus: None,
        memory_mb: None,
        name: None,
        network: None,
        idle_timeout_secs: None,
        env: vec![],
    };
    let create = handle_create(req, &cfg, &h.registry, &*h.launcher, |_| {})
        .await
        .unwrap();
    let sid = create.sandbox_id;

    let exec = handle_exec(
        ExecRequest {
            sandbox_id: sid.clone(),
            cmd: "/bin/echo".into(),
            args: vec!["hi".into()],
            stdin: None,
            env: vec![],
            timeout_ms: None,
            workdir: None,
        },
        &h.registry,
        &*h.runner,
    )
    .await
    .unwrap();
    assert_eq!(exec.stdout, "hi\n");
    assert_eq!(exec.exit_code, Some(0));

    let stop = handle_stop(
        StopRequest {
            sandbox_id: sid,
            wait: true,
        },
        &h.registry,
        &*h.stopper,
    )
    .await
    .unwrap();
    assert!(stop.stopped);
    assert_eq!(h.stopper.stops.load(Ordering::SeqCst), 1);
}

#[tokio::test]
#[serial]
async fn concurrent_exec_returns_s003() {
    let _guard = ensure_fake_rootfs("python");
    let h = harness("", 0);
    let cfg = cfg_all_presets_allowed();
    let create = handle_create(
        CreateRequest {
            image: "python".into(),
            cpus: None,
            memory_mb: None,
            name: None,
            network: None,
            idle_timeout_secs: None,
            env: vec![],
        },
        &cfg,
        &h.registry,
        &*h.launcher,
        |_| {},
    )
    .await
    .unwrap();

    let id = uuid::Uuid::parse_str(&create.sandbox_id).unwrap();
    h.registry.begin_exec(id).await.unwrap();

    let err = handle_exec(
        ExecRequest {
            sandbox_id: create.sandbox_id,
            cmd: "/bin/true".into(),
            args: vec![],
            stdin: None,
            env: vec![],
            timeout_ms: None,
            workdir: None,
        },
        &h.registry,
        &*h.runner,
    )
    .await
    .unwrap_err();
    assert_eq!(err.code().as_str(), "S003");
}

#[tokio::test]
#[serial]
async fn image_not_allowlisted_returns_s100() {
    let h = harness("", 0);
    let mut cfg = cfg_all_presets_allowed();
    cfg.image_allowlist = vec!["python".into()];
    let req = CreateRequest {
        image: "node".into(),
        cpus: None,
        memory_mb: None,
        name: None,
        network: None,
        idle_timeout_secs: None,
        env: vec![],
    };
    let err = handle_create(req, &cfg, &h.registry, &*h.launcher, |_| {})
        .await
        .unwrap_err();
    assert_eq!(err.code().as_str(), "S100");
}

#[tokio::test]
#[serial]
async fn rootfs_missing_no_autoinstall_returns_s101() {
    let td = tempfile::tempdir().unwrap();
    let orig = std::env::var_os("HOME");
    // SAFETY: test is annotated with #[serial]; no other threads mutate env.
    unsafe {
        std::env::set_var("HOME", td.path());
    }
    let h = harness("", 0);
    let mut cfg = cfg_all_presets_allowed();
    cfg.auto_install = false;
    let err = handle_create(
        CreateRequest {
            image: "python".into(),
            cpus: None,
            memory_mb: None,
            name: None,
            network: None,
            idle_timeout_secs: None,
            env: vec![],
        },
        &cfg,
        &h.registry,
        &*h.launcher,
        |_| {},
    )
    .await
    .unwrap_err();
    assert_eq!(err.code().as_str(), "S101");
    // SAFETY: test is annotated with #[serial]; no other threads mutate env.
    unsafe {
        match orig {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
    }
}

#[tokio::test]
#[serial]
async fn memory_cap_returns_s400() {
    let _guard = ensure_fake_rootfs("python");
    let h = harness("", 0);
    let mut cfg = cfg_all_presets_allowed();
    cfg.per_image_caps.insert(
        "python".into(),
        iii_worker::sandbox_daemon::config::PerImageCap {
            max_cpus: 4,
            max_memory_mb: 1024,
        },
    );
    let err = handle_create(
        CreateRequest {
            image: "python".into(),
            cpus: None,
            memory_mb: Some(9999),
            name: None,
            network: None,
            idle_timeout_secs: None,
            env: vec![],
        },
        &cfg,
        &h.registry,
        &*h.launcher,
        |_| {},
    )
    .await
    .unwrap_err();
    assert_eq!(err.code().as_str(), "S400");
}

#[tokio::test]
#[serial]
async fn list_returns_every_sandbox() {
    let _guard = ensure_fake_rootfs("python");
    let h = harness("", 0);
    let cfg = cfg_all_presets_allowed();
    let _ = handle_create(
        CreateRequest {
            image: "python".into(),
            cpus: None,
            memory_mb: None,
            name: None,
            network: None,
            idle_timeout_secs: None,
            env: vec![],
        },
        &cfg,
        &h.registry,
        &*h.launcher,
        |_| {},
    )
    .await
    .unwrap();
    let _ = handle_create(
        CreateRequest {
            image: "python".into(),
            cpus: None,
            memory_mb: None,
            name: None,
            network: None,
            idle_timeout_secs: None,
            env: vec![],
        },
        &cfg,
        &h.registry,
        &*h.launcher,
        |_| {},
    )
    .await
    .unwrap();

    use iii_worker::sandbox_daemon::list::{ListRequest, handle_list};
    let resp = handle_list(ListRequest::default(), &h.registry).await;
    assert_eq!(resp.sandboxes.len(), 2);
}

struct RootfsGuard(
    #[allow(dead_code)] tempfile::TempDir,
    Option<std::ffi::OsString>,
);
impl Drop for RootfsGuard {
    fn drop(&mut self) {
        // SAFETY: guard only exists inside #[serial] tests; no other threads
        // mutate env while it is alive.
        unsafe {
            match &self.1 {
                Some(v) => std::env::set_var("HOME", v),
                None => std::env::remove_var("HOME"),
            }
        }
    }
}
fn ensure_fake_rootfs(image: &str) -> RootfsGuard {
    let td = tempfile::tempdir().unwrap();
    let rootfs = td
        .path()
        .join(".iii")
        .join("managed")
        .join(image)
        .join("rootfs");
    // `bin/` is required by overlay::lower_rootfs_exists to treat the
    // rootfs as populated -- bare dir was previously accepted and masked
    // a class of auto_install silent-no-op bugs.
    std::fs::create_dir_all(rootfs.join("bin")).unwrap();
    let orig = std::env::var_os("HOME");
    // SAFETY: callers wrap in #[serial] so no other threads mutate env.
    unsafe {
        std::env::set_var("HOME", td.path());
    }
    RootfsGuard(td, orig)
}
