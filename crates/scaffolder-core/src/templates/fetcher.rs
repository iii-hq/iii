//! Template fetching from remote (GitHub templates repo) or local directory
//!
//! Templates are stored as plain files in a dedicated repository (iii-hq/templates).
//! - Remote: fetches individual files over raw HTTPS from the templates repo
//! - Local: reads files directly from disk (for development use)

use super::manifest::{RootManifest, SharedFile, TemplateManifest};
use crate::product::ProductConfig;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::fs;
use url::Url;

/// Template source - either remote URL or local directory
#[derive(Debug, Clone)]
pub enum TemplateSource {
    Remote(Url),
    Local(PathBuf),
}

impl TemplateSource {
    /// Create a remote template source from a product config
    pub fn from_config<C: ProductConfig>(config: &C) -> Result<Self> {
        let url_str = std::env::var(config.template_url_env())
            .unwrap_or_else(|_| config.default_template_url().to_string());
        let url =
            Url::parse(&url_str).with_context(|| format!("Invalid template URL: {}", url_str))?;
        Ok(Self::Remote(url))
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

/// Template fetcher - handles retrieving templates from remote or local sources
pub struct TemplateFetcher {
    source: TemplateSource,
    client: reqwest::Client,
    /// Cached root manifest (fetched once per session)
    root_manifest_cache: Option<RootManifest>,
    /// Cache of loaded templates
    template_cache: HashMap<String, TemplateCache>,
}

impl TemplateFetcher {
    /// Create a new fetcher with a custom user agent
    pub fn new(source: TemplateSource, user_agent: &str) -> Self {
        Self {
            source,
            client: reqwest::Client::builder()
                .user_agent(user_agent)
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
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

    /// Build a URL by appending a path segment, preserving query parameters
    fn build_url(base: &Url, path_segment: &str) -> Result<Url> {
        let mut url = base.clone();
        url.path_segments_mut()
            .map_err(|_| anyhow::anyhow!("URL cannot have path segments: {}", base))?
            .pop_if_empty();
        for part in path_segment.split('/').filter(|s| !s.is_empty()) {
            url.path_segments_mut()
                .map_err(|_| anyhow::anyhow!("URL cannot have path segments: {}", base))?
                .push(part);
        }
        Ok(url)
    }

    async fn get_bytes(&self, url: &Url) -> Result<Vec<u8>> {
        let response = self
            .client
            .get(url.clone())
            .send()
            .await
            .with_context(|| format!("Failed to fetch {}", url))?;
        if !response.status().is_success() {
            anyhow::bail!("Failed to fetch {}: HTTP {}", url, response.status());
        }
        Ok(response.bytes().await?.to_vec())
    }

    async fn get_string(&self, url: &Url) -> Result<String> {
        let bytes = self.get_bytes(url).await?;
        String::from_utf8(bytes).with_context(|| format!("{} is not valid UTF-8", url))
    }

    /// Fetch the root manifest listing available templates
    pub async fn fetch_root_manifest(&mut self) -> Result<RootManifest> {
        if let Some(cached) = &self.root_manifest_cache {
            return Ok(cached.clone());
        }

        let manifest: RootManifest = match &self.source {
            TemplateSource::Remote(base_url) => {
                let url = Self::build_url(base_url, "template.yaml")?;
                let content = self.get_string(&url).await.with_context(|| {
                    format!("Failed to fetch root template manifest from {}", url)
                })?;
                serde_yaml::from_str(&content).context("Failed to parse root manifest")?
            }
            TemplateSource::Local(path) => {
                let manifest_path = path.join("template.yaml");
                let content = fs::read_to_string(&manifest_path)
                    .await
                    .with_context(|| format!("Failed to read {}", manifest_path.display()))?;
                serde_yaml::from_str(&content).context("Failed to parse root manifest")?
            }
        };

        self.root_manifest_cache = Some(manifest.clone());
        Ok(manifest)
    }

    /// Load a file for a template, applying shared-file renames
    /// `file_path` is the path relative to the template (what will live on disk in the project)
    async fn load_template_file(
        &self,
        template_name: &str,
        file_path: &str,
        shared_files: &[SharedFile],
    ) -> Result<Vec<u8>> {
        // If this file matches a shared-file destination, read from the shared source instead
        let shared_source: Option<&str> = shared_files
            .iter()
            .find(|s| s.destination() == file_path)
            .map(|s| s.source.as_str());

        match &self.source {
            TemplateSource::Remote(base_url) => {
                let url = if let Some(source) = shared_source {
                    Self::build_url(base_url, source)?
                } else {
                    Self::build_url(base_url, &format!("{}/{}", template_name, file_path))?
                };
                self.get_bytes(&url).await
            }
            TemplateSource::Local(dir) => {
                let full_path = if let Some(source) = shared_source {
                    dir.join(source)
                } else {
                    dir.join(template_name).join(file_path)
                };
                fs::read(&full_path)
                    .await
                    .with_context(|| format!("Failed to read {}", full_path.display()))
            }
        }
    }

    async fn fetch_template_manifest_raw(&self, template_name: &str) -> Result<TemplateManifest> {
        match &self.source {
            TemplateSource::Remote(base_url) => {
                let url =
                    Self::build_url(base_url, &format!("{}/template.yaml", template_name))?;
                let content = self.get_string(&url).await.with_context(|| {
                    format!("Failed to fetch template '{}' manifest", template_name)
                })?;
                serde_yaml::from_str(&content).with_context(|| {
                    format!("Failed to parse template '{}' manifest", template_name)
                })
            }
            TemplateSource::Local(dir) => {
                let path = dir.join(template_name).join("template.yaml");
                let content = fs::read_to_string(&path)
                    .await
                    .with_context(|| format!("Failed to read {}", path.display()))?;
                serde_yaml::from_str(&content).with_context(|| {
                    format!("Failed to parse template '{}' manifest", template_name)
                })
            }
        }
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

        let mut files: HashMap<String, Vec<u8>> = HashMap::new();
        for file_path in &manifest.files {
            match self
                .load_template_file(template_name, file_path, &root_manifest.shared_files)
                .await
            {
                Ok(bytes) => {
                    files.insert(file_path.clone(), bytes);
                }
                Err(err) => {
                    // Warn but don't fail - file might be optional
                    eprintln!(
                        "Warning: could not load '{}' from template '{}': {}",
                        file_path, template_name, err
                    );
                }
            }
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
