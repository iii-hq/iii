//! Scaffolder Core - Shared library for project scaffolding CLIs
//!
//! This library provides the core functionality for scaffolding projects from templates.
//! It is designed to be used by multiple CLI binaries (e.g., `motia`, `iii`) that share
//! the same underlying scaffolding logic but have different product configurations.
//!
//! # Architecture
//!
//! - **Core operations:** template fetching, copying, runtime detection, telemetry
//! - **Workflow orchestration:** `ProductConfig` trait describing each product
//! - **CLI/TUI interface:** cliclack-based prompts exposed via `run`

pub mod product;
pub mod runtime;
pub mod telemetry;
pub mod templates;
pub mod tui;

// Re-export main types for convenience
pub use product::ProductConfig;
pub use runtime::{check_runtimes, Language, RuntimeInfo};
pub use templates::{
    copy_template, LanguageFiles, RootManifest, TemplateFetcher, TemplateManifest, TemplateSource,
};
pub use tui::run;

/// CLI version - used for template compatibility checking
/// Each binary should define its own version, but this provides a fallback
pub const DEFAULT_CLI_VERSION: &str = "0.1.0";
