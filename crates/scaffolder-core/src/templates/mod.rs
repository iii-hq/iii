//! Template fetching, parsing, and copying
//!
//! This module provides:
//! - Template manifest types (RootManifest, TemplateManifest)
//! - Template fetching from remote (templates repo) or local directories
//! - Template copying with language-based filtering
//! - Version compatibility checking

pub mod copier;
pub mod fetcher;
pub mod manifest;
pub mod version;

pub use copier::copy_template;
pub use fetcher::{TemplateFetcher, TemplateSource};
pub use manifest::{LanguageFiles, RootManifest, SharedFile, TemplateManifest};
pub use version::check_compatibility;
