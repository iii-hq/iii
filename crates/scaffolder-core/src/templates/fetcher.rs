//! Template fetching from a git repo (cloned on demand) or a local directory.
//!
//! - Remote: clones the templates repo (shallow) to a tempdir on first use,
//!   then reads files from the subdirectory that matches the product
//!   (`iii/`, `motia/`, etc.). The tempdir is cleaned up when the fetcher is
//!   dropped.
//! - Local: reads files directly from disk (for development use, via
//!   `--template-dir`). The path should already point at the product
//!   subdirectory.

use super::manifest::{RootManifest, SharedFile, TemplateManifest};
use crate::product::ProductConfig;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tempfile::TempDir;
use tokio::fs;
use tokio::process::Command;
use url::Url;

/// Default branch when the env override doesn't specify one
const DEFAULT_BRANCH: &str = "main";

/// Template source - either a git repo to clone or a local directory
#[derive(Debug, Clone)]
pub enum TemplateSource {
    /// Clone this git URL and read files from `{clone_root}/{subdir}`
    Remote {
        repo_url: Url,
        subdir: String,
        branch: String,
    },
    /// Read files directly from this directory (which already points at the
    /// product subdirectory, e.g. `.../templates/iii`)
    Local(PathBuf),
}

impl TemplateSource {
    /// Create a remote template source from a product config
    pub fn from_config<C: ProductConfig>(config: &C) -> Result<Self> {
        let url_str = std::env::var(config.template_url_env())
            .unwrap_or_else(|_| config.default_template_url().to_string());
        let repo_url = Url::parse(&url_str)
            .with_context(|| format!("Invalid template repo URL: {}", url_str))?;
        let branch = std::env::var(format!("{}_BRANCH", config.template_url_env()))
            .unwrap_or_else(|_| DEFAULT_BRANCH.to_string());
        Ok(Self::Remote {
            repo_url,
            subdir: config.name().to_string(),
            branch,
        })
    }

    /// Create a local template source from a path
    pub fn local(path: PathBuf) -> Self {
        Self::Local(path)
    }
}

/// Cached template data
#[derive(Debug, Clone)]
struct TemplateCache {
    manifest: TemplateManifest,
    files: HashMap<String, Vec<u8>>,
}

/// Template fetcher - handles retrieving templates from a git repo or a
/// local directory. For Remote sources the repo is shallow-cloned to a
/// tempdir on first use; subsequent file reads are served from disk.
pub struct TemplateFetcher {
    source: TemplateSource,
    /// Materialized local path to read from (set after first clone or at
    /// construction time for Local sources)
    effective_path: Option<PathBuf>,
    /// Keeps the shallow clone alive for the lifetime of the fetcher. When
    /// dropped, the tempdir is removed.
    _clone_guard: Option<TempDir>,
    /// Cached root manifest (parsed once per session)
    root_manifest_cache: Option<RootManifest>,
    /// Cache of loaded templates
    template_cache: HashMap<String, TemplateCache>,
}

impl TemplateFetcher {
    /// Create a new fetcher. `_user_agent` is retained for API compatibility
    /// but is no longer used (clones go through the system `git` binary,
    /// which uses its own user agent).
    pub fn new(source: TemplateSource, _user_agent: &str) -> Self {
        let effective_path = match &source {
            TemplateSource::Local(p) => Some(p.clone()),
            TemplateSource::Remote { .. } => None,
        };
        Self {
            source,
            effective_path,
            _clone_guard: None,
            root_manifest_cache: None,
            template_cache: HashMap::new(),
        }
    }

    /// Create a fetcher from a product config
    pub fn from_config<C: ProductConfig>(config: &C) -> Result<Self> {
        let source = TemplateSource::from_config(config)?;
        Ok(Self::new(source, config.user_agent()))
    }

    /// Create a fetcher for local templates
    pub fn from_local(path: PathBuf, user_agent: &str) -> Self {
        Self::new(TemplateSource::local(path), user_agent)
    }

    /// Ensure the fetcher has a local directory to read from.
    /// For Remote sources, clone the repo on first call.
    async fn ensure_local(&mut self) -> Result<&Path> {
        if self.effective_path.is_some() {
            return Ok(self.effective_path.as_deref().unwrap());
        }

        let TemplateSource::Remote {
            repo_url,
            subdir,
            branch,
        } = &self.source
        else {
            anyhow::bail!("internal: Local source must have effective_path set");
        };

        check_git_available().await?;

        let tmp = TempDir::new().context("Failed to create temp directory for clone")?;
        let output = Command::new("git")
            .arg("clone")
            .arg("--depth")
            .arg("1")
            .arg("--branch")
            .arg(branch)
            .arg("--single-branch")
            .arg(repo_url.as_str())
            .arg(tmp.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .context("Failed to run git clone")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!(
                "git clone of {} (branch {}) failed: {}",
                repo_url,
                branch,
                stderr.trim()
            );
        }

        let product_path = tmp.path().join(subdir);
        if !product_path.exists() {
            anyhow::bail!(
                "Subdirectory '{}' not found in cloned repo {}",
                subdir,
                repo_url
            );
        }

        self.effective_path = Some(product_path);
        self._clone_guard = Some(tmp);
        Ok(self.effective_path.as_deref().unwrap())
    }

    /// Fetch the root manifest listing available templates
    pub async fn fetch_root_manifest(&mut self) -> Result<RootManifest> {
        if let Some(cached) = &self.root_manifest_cache {
            return Ok(cached.clone());
        }
        let base = self.ensure_local().await?;
        let manifest_path = base.join("template.yaml");
        let content = fs::read_to_string(&manifest_path)
            .await
            .with_context(|| format!("Failed to read {}", manifest_path.display()))?;
        let manifest: RootManifest =
            serde_yaml::from_str(&content).context("Failed to parse root manifest")?;
        self.root_manifest_cache = Some(manifest.clone());
        Ok(manifest)
    }

    async fn fetch_template_manifest_raw(
        &mut self,
        template_name: &str,
    ) -> Result<TemplateManifest> {
        let base = self.ensure_local().await?;
        let path = base.join(template_name).join("template.yaml");
        let content = fs::read_to_string(&path)
            .await
            .with_context(|| format!("Failed to read {}", path.display()))?;
        serde_yaml::from_str(&content)
            .with_context(|| format!("Failed to parse template '{}' manifest", template_name))
    }

    async fn load_template_file(
        base: &Path,
        template_name: &str,
        file_path: &str,
        shared_files: &[SharedFile],
    ) -> Result<Vec<u8>> {
        // If this file matches a shared-file destination, read from the shared source instead
        let shared_source: Option<&str> = shared_files
            .iter()
            .find(|s| s.destination() == file_path)
            .map(|s| s.source.as_str());
        let full_path = if let Some(src) = shared_source {
            base.join(src)
        } else {
            base.join(template_name).join(file_path)
        };
        fs::read(&full_path)
            .await
            .with_context(|| format!("Failed to read {}", full_path.display()))
    }

    /// Load and cache a template (manifest + all listed files, including shared files)
    async fn load_template(&mut self, template_name: &str) -> Result<()> {
        if self.template_cache.contains_key(template_name) {
            return Ok(());
        }

        let root_manifest = self.fetch_root_manifest().await?;
        let mut manifest = self.fetch_template_manifest_raw(template_name).await?;

        // Shared files expand the file list so they are treated uniformly during
        // language filtering and copying.
        for shared in &root_manifest.shared_files {
            let dest = shared.destination().to_string();
            if !manifest.files.contains(&dest) {
                manifest.files.push(dest);
            }
        }

        let base = self.ensure_local().await?.to_path_buf();
        let shared_files = root_manifest.shared_files.clone();
        let mut files: HashMap<String, Vec<u8>> = HashMap::new();
        for file_path in &manifest.files {
            let bytes = Self::load_template_file(&base, template_name, file_path, &shared_files)
                .await
                .with_context(|| {
                    format!(
                        "Failed to load '{}' from template '{}'",
                        file_path, template_name
                    )
                })?;
            files.insert(file_path.clone(), bytes);
        }

        self.template_cache
            .insert(template_name.to_string(), TemplateCache { manifest, files });
        Ok(())
    }

    /// Fetch a specific template's manifest
    pub async fn fetch_template_manifest(
        &mut self,
        template_name: &str,
    ) -> Result<TemplateManifest> {
        self.load_template(template_name).await?;
        let cache = self
            .template_cache
            .get(template_name)
            .ok_or_else(|| anyhow::anyhow!("Template '{}' not found in cache", template_name))?;
        Ok(cache.manifest.clone())
    }

    /// Fetch a specific file from a template as string
    #[allow(dead_code)]
    pub async fn fetch_file(&mut self, template_name: &str, file_path: &str) -> Result<String> {
        let bytes = self.fetch_file_bytes(template_name, file_path).await?;
        String::from_utf8(bytes).context("File is not valid UTF-8")
    }

    /// Fetch a file as bytes (for binary files)
    pub async fn fetch_file_bytes(
        &mut self,
        template_name: &str,
        file_path: &str,
    ) -> Result<Vec<u8>> {
        self.load_template(template_name).await?;
        let cache = self
            .template_cache
            .get(template_name)
            .ok_or_else(|| anyhow::anyhow!("Template '{}' not found in cache", template_name))?;
        cache.files.get(file_path).cloned().ok_or_else(|| {
            anyhow::anyhow!(
                "File '{}' not found in template '{}'",
                file_path,
                template_name
            )
        })
    }

    /// Get the template source
    #[allow(dead_code)]
    pub fn source(&self) -> &TemplateSource {
        &self.source
    }
}

async fn check_git_available() -> Result<()> {
    let output = Command::new("git")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await;
    match output {
        Ok(status) if status.success() => Ok(()),
        _ => anyhow::bail!(
            "git is required to fetch templates but was not found on PATH. \
             Install git (https://git-scm.com/downloads) or pass --template-dir \
             to use a local template directory."
        ),
    }
}
