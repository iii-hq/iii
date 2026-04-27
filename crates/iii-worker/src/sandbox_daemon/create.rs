use crate::cli::rootfs_cache;
use crate::sandbox_daemon::config::SandboxConfig;
use crate::sandbox_daemon::{
    auto_install, catalog,
    errors::SandboxError,
    events::SandboxCreateEvent,
    overlay::OverlayLayout,
    registry::{SandboxRegistry, SandboxState},
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Instant;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct CreateRequest {
    pub image: String,
    #[serde(default)]
    pub cpus: Option<u32>,
    #[serde(default)]
    pub memory_mb: Option<u32>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub network: Option<bool>,
    #[serde(default)]
    pub idle_timeout_secs: Option<u64>,
    #[serde(default)]
    pub env: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct CreateResponse {
    pub sandbox_id: String,
    pub image: String,
}

#[async_trait::async_trait]
pub trait VmLauncher: Send + Sync + 'static {
    async fn boot(&self, params: &BootParams) -> Result<BootHandle, SandboxError>;
}

pub struct BootParams {
    pub rootfs: PathBuf,
    pub workdir: PathBuf,
    pub shell_sock: PathBuf,
    pub cpus: u32,
    pub memory_mb: u32,
    pub env: Vec<String>,
    pub network: bool,
}

pub struct BootHandle {
    pub vm_pid: u32,
}

pub async fn handle_create<L: VmLauncher, F: FnMut(SandboxCreateEvent) + Send + 'static>(
    req: CreateRequest,
    cfg: &SandboxConfig,
    registry: &SandboxRegistry,
    launcher: &L,
    mut on_event: F,
) -> Result<CreateResponse, SandboxError> {
    let t_create = Instant::now();
    if registry.count().await >= cfg.max_concurrent_sandboxes as usize {
        return Err(SandboxError::ResourceLimit(format!(
            "max_concurrent_sandboxes ({}) reached",
            cfg.max_concurrent_sandboxes
        )));
    }

    // `check_allowlist` already proved the image is known, so
    // `resolve_image` cannot miss here.
    catalog::check_allowlist(&req.image, &cfg.image_allowlist, &cfg.custom_images)?;
    let oci_ref = catalog::resolve_image(&req.image, &cfg.custom_images)
        .expect("allowlist guards against unknown images");

    let cpus = req.cpus.unwrap_or(cfg.default_cpus);
    let memory_mb = req.memory_mb.unwrap_or(cfg.default_memory_mb);
    if let Some(cap) = cfg.per_image_caps.get(&req.image) {
        if cpus > cap.max_cpus {
            return Err(SandboxError::ResourceLimit(format!(
                "cpus={} exceeds per-image cap {}",
                cpus, cap.max_cpus
            )));
        }
        if memory_mb > cap.max_memory_mb {
            return Err(SandboxError::ResourceLimit(format!(
                "memory_mb={} exceeds per-image cap {}",
                memory_mb, cap.max_memory_mb
            )));
        }
    }

    // Rootfs path may come from the unified cache
    // (`~/.iii/cache/<slug>/`) or a legacy sandbox path
    // (`~/.iii/managed/<image>/rootfs/`) consulted as a fallback so
    // pre-unification rootfses stay hot.
    let hints = rootfs_cache::CacheHints {
        legacy_preset: Some(&req.image),
        ..Default::default()
    };
    let rootfs = match rootfs_cache::resolve_cached(&oci_ref, &hints) {
        Some(path) => path,
        None => {
            if !cfg.auto_install {
                return Err(SandboxError::RootfsMissing {
                    image: req.image.clone(),
                });
            }
            let t_ai = Instant::now();
            // auto_install_image requires `impl FnMut(SandboxCreateEvent) + Send + 'static`.
            // We can't borrow on_event into it (lifetime), so we buffer events
            // through a channel: the Sender is Send+'static, the future is
            // awaited inline, then we drain the channel and forward to on_event
            // before continuing.
            let (tx, rx) = std::sync::mpsc::channel::<SandboxCreateEvent>();
            let dest = auto_install::auto_install_image(&req.image, &oci_ref, move |e| {
                let _ = tx.send(e);
            })
            .await?;
            for e in rx.try_iter() {
                on_event(e);
            }
            tracing::info!(
                ms = t_ai.elapsed().as_millis() as u64,
                image = %req.image,
                rootfs = %dest.display(),
                "create_phase: auto_install (pull+extract)"
            );
            dest
        }
    };

    let id = Uuid::new_v4();
    let layout = OverlayLayout::for_sandbox(id);
    layout
        .ensure_dirs()
        .map_err(|e| SandboxError::BootFailed(format!("overlay dirs: {e}")))?;
    let shell_sock = layout.base().join("shell.sock");
    let workdir = layout.merged.clone();

    on_event(SandboxCreateEvent::BootingVm);

    let boot = launcher
        .boot(&BootParams {
            rootfs: rootfs.clone(),
            workdir: workdir.clone(),
            shell_sock: shell_sock.clone(),
            cpus,
            memory_mb,
            env: req.env.clone(),
            network: req.network.unwrap_or(false),
        })
        .await?;

    let state = SandboxState {
        id,
        name: req.name.clone(),
        image: req.image.clone(),
        rootfs,
        workdir,
        shell_sock,
        vm_pid: Some(boot.vm_pid),
        created_at: Instant::now(),
        last_exec_at: Instant::now(),
        exec_in_progress: false,
        idle_timeout_secs: req
            .idle_timeout_secs
            .unwrap_or(cfg.default_idle_timeout_secs),
        stopped: false,
    };
    registry.insert(state).await;

    on_event(SandboxCreateEvent::Ready {
        sandbox_id: id.to_string(),
    });

    tracing::info!(
        ms = t_create.elapsed().as_millis() as u64,
        sandbox_id = %id,
        image = %req.image,
        "create_phase: handle_create TOTAL"
    );

    Ok(CreateResponse {
        sandbox_id: id.to_string(),
        image: req.image,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeLauncher {
        vm_pid: u32,
    }
    #[async_trait::async_trait]
    impl VmLauncher for FakeLauncher {
        async fn boot(&self, _p: &BootParams) -> Result<BootHandle, SandboxError> {
            Ok(BootHandle {
                vm_pid: self.vm_pid,
            })
        }
    }

    fn cfg_with_python_allowed() -> SandboxConfig {
        SandboxConfig {
            auto_install: false,
            image_allowlist: vec!["python".into()],
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn image_not_in_allowlist_returns_s100() {
        let cfg = cfg_with_python_allowed();
        let reg = SandboxRegistry::new();
        let launcher = FakeLauncher { vm_pid: 1 };
        let req = CreateRequest {
            image: "malicious".into(),
            cpus: None,
            memory_mb: None,
            name: None,
            network: None,
            idle_timeout_secs: None,
            env: vec![],
        };
        let err = handle_create(req, &cfg, &reg, &launcher, |_| {})
            .await
            .unwrap_err();
        assert_eq!(err.code().as_str(), "S100");
    }

    #[tokio::test]
    async fn custom_image_not_in_allowlist_returns_s100() {
        // custom_images entry exists, but it is not in image_allowlist.
        // Known-to-daemon ≠ permitted. Must still return S100.
        let mut cfg = cfg_with_python_allowed();
        cfg.custom_images
            .insert("my-app".into(), "ghcr.io/acme/my-app:1".into());
        let reg = SandboxRegistry::new();
        let launcher = FakeLauncher { vm_pid: 1 };
        let req = CreateRequest {
            image: "my-app".into(),
            cpus: None,
            memory_mb: None,
            name: None,
            network: None,
            idle_timeout_secs: None,
            env: vec![],
        };
        let err = handle_create(req, &cfg, &reg, &launcher, |_| {})
            .await
            .unwrap_err();
        assert_eq!(err.code().as_str(), "S100");
    }

    #[tokio::test]
    async fn cpu_cap_exceeded_returns_s400() {
        let mut cfg = cfg_with_python_allowed();
        cfg.per_image_caps.insert(
            "python".into(),
            crate::sandbox_daemon::config::PerImageCap {
                max_cpus: 2,
                max_memory_mb: 1024,
            },
        );
        let reg = SandboxRegistry::new();
        let launcher = FakeLauncher { vm_pid: 1 };
        let req = CreateRequest {
            image: "python".into(),
            cpus: Some(99),
            memory_mb: None,
            name: None,
            network: None,
            idle_timeout_secs: None,
            env: vec![],
        };
        let err = handle_create(req, &cfg, &reg, &launcher, |_| {})
            .await
            .unwrap_err();
        assert_eq!(err.code().as_str(), "S400");
    }
}
