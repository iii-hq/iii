//! Product configuration trait for CLI binaries
//!
//! This trait defines the interface that each product (motia, iii) must implement
//! to configure the scaffolding behavior for their specific needs.

/// Configuration trait for different CLI products
///
/// Each product (motia, iii) implements this trait to define:
/// - Product identity (name, display name)
/// - Template source URLs
/// - Tool dependencies
/// - Documentation links
pub trait ProductConfig: Clone + Send + Sync + 'static {
    /// Internal product name (used for CLI command, env vars)
    fn name(&self) -> &'static str;

    /// Human-readable display name
    fn display_name(&self) -> &'static str;

    /// Default URL for fetching templates
    fn default_template_url(&self) -> &'static str;

    /// Environment variable name for overriding template URL
    fn template_url_env(&self) -> &'static str;

    /// Whether this product requires the iii tool to be installed
    fn requires_iii(&self) -> bool;

    /// URL for product documentation
    fn docs_url(&self) -> &'static str;

    /// CLI description shown in help text
    fn cli_description(&self) -> &'static str;

    /// Upgrade/install command shown in version warnings
    fn upgrade_command(&self) -> &'static str;

    /// User agent string for HTTP requests
    fn user_agent(&self) -> &'static str {
        self.name()
    }
}
