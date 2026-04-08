// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

pub mod adapter;
#[cfg(all(target_os = "linux", not(target_env = "musl")))]
pub mod libkrun;
pub mod oci;
pub mod platform;
pub mod state;

use std::sync::Arc;

use self::adapter::RuntimeAdapter;

/// Create the runtime adapter.
#[cfg(all(target_os = "linux", not(target_env = "musl")))]
pub fn create_adapter(_runtime: &str) -> Arc<dyn RuntimeAdapter> {
    Arc::new(libkrun::LibkrunAdapter::new())
}

/// Stub adapter for platforms where VM sandbox is not available.
#[cfg(not(all(target_os = "linux", not(target_env = "musl"))))]
pub fn create_adapter(_runtime: &str) -> Arc<dyn RuntimeAdapter> {
    Arc::new(UnsupportedAdapter)
}

#[cfg(not(all(target_os = "linux", not(target_env = "musl"))))]
struct UnsupportedAdapter;

#[cfg(not(all(target_os = "linux", not(target_env = "musl"))))]
#[async_trait::async_trait]
impl RuntimeAdapter for UnsupportedAdapter {
    async fn pull(&self, _image: &str) -> anyhow::Result<adapter::ImageInfo> {
        anyhow::bail!("VM sandbox is not available on this platform")
    }
    async fn extract_file(&self, _image: &str, _path: &str) -> anyhow::Result<Vec<u8>> {
        anyhow::bail!("VM sandbox is not available on this platform")
    }
    async fn start(&self, _spec: &adapter::ContainerSpec) -> anyhow::Result<String> {
        anyhow::bail!("VM sandbox is not available on this platform")
    }
    async fn stop(&self, _container_id: &str, _timeout_secs: u32) -> anyhow::Result<()> {
        anyhow::bail!("VM sandbox is not available on this platform")
    }
    async fn status(&self, _container_id: &str) -> anyhow::Result<adapter::ContainerStatus> {
        anyhow::bail!("VM sandbox is not available on this platform")
    }
    async fn remove(&self, _container_id: &str) -> anyhow::Result<()> {
        anyhow::bail!("VM sandbox is not available on this platform")
    }
}

/// Check if libkrun runtime is available on this platform.
#[cfg(all(target_os = "linux", not(target_env = "musl")))]
pub fn sandbox_available() -> bool {
    libkrun::libkrun_available()
}

/// Sandbox is never available on non-Linux GNU platforms.
#[cfg(not(all(target_os = "linux", not(target_env = "musl"))))]
pub fn sandbox_available() -> bool {
    false
}
