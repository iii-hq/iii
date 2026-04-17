//! Scaffolder Core - Shared library for project scaffolding CLIs
//!
//! This library provides the core functionality for scaffolding projects from templates.
//! It is designed to be used by multiple CLI binaries (e.g., `motia`, `iii`) that share
//! the same underlying scaffolding logic but have different product configurations.
//!
//! # Architecture
//!
//! The library is organized into layers:
//!
//! - **Layer 1: Core Operations** - Pure functions for template fetching, copying, runtime detection
//! - **Layer 2: Workflow Orchestration** - `ProductConfig` trait and `ProjectBuilder` for custom UIs
//! - **Layer 3: CLI/TUI Interface** - Optional cliclack-based prompts (feature-gated)
//!
//! # Feature Flags
//!
//! - `tui` (default): Enables the cliclack-based TUI prompts module
//!
//! # Example Usage (without TUI)
//!
//! ```rust,no_run
//! use scaffolder_core::{ProductConfig, TemplateFetcher};
//!
//! #[derive(Clone)]
//! struct MyConfig;
//!
//! impl ProductConfig for MyConfig {
//!     fn name(&self) -> &'static str { "myapp" }
//!     fn display_name(&self) -> &'static str { "My App" }
//!     fn default_template_url(&self) -> &'static str { "https://example.com/templates" }
//!     fn template_url_env(&self) -> &'static str { "MYAPP_TEMPLATE_URL" }
//!     fn requires_iii(&self) -> bool { true }
//!     fn docs_url(&self) -> &'static str { "https://example.com/docs" }
//!     fn cli_description(&self) -> &'static str { "My scaffolder" }
//!     fn upgrade_command(&self) -> &'static str { "cargo install myapp" }
//! }
//!
//! let fetcher = TemplateFetcher::from_config(&MyConfig).unwrap();
//! ```

pub mod product;
pub mod runtime;
pub mod telemetry;
pub mod templates;

#[cfg(feature = "tui")]
pub mod tui;

// Re-export main types for convenience
pub use product::ProductConfig;
pub use runtime::{check_runtimes, Language, RuntimeInfo};
pub use templates::{
    copy_template, LanguageFiles, RootManifest, TemplateFetcher, TemplateManifest, TemplateSource,
};

#[cfg(feature = "tui")]
pub use tui::run;

/// CLI version - used for template compatibility checking
/// Each binary should define its own version, but this provides a fallback
pub const DEFAULT_CLI_VERSION: &str = "0.1.0";
