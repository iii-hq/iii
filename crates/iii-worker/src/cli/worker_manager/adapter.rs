use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageInfo {
    pub image: String,
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerSpec {
    pub name: String,
    pub image: String,
    pub env: HashMap<String, String>,
    pub memory_limit: Option<String>,
    pub cpu_limit: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerStatus {
    pub name: String,
    pub container_id: String,
    pub running: bool,
    pub exit_code: Option<i32>,
}

#[async_trait::async_trait]
pub trait RuntimeAdapter: Send + Sync {
    async fn pull(&self, image: &str) -> Result<ImageInfo>;
    async fn extract_file(&self, image: &str, path: &str) -> Result<Vec<u8>>;
    async fn start(&self, spec: &ContainerSpec) -> Result<String>;
    async fn stop(&self, container_id: &str, timeout_secs: u32) -> Result<()>;
    async fn status(&self, container_id: &str) -> Result<ContainerStatus>;
    async fn remove(&self, container_id: &str) -> Result<()>;
}
