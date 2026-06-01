//! iii-specific [`ProductConfig`] implementation.
//!
//! Lives here (rather than in the engine binary) so that any future caller
//! who wants to drive the scaffolder against the iii product reuses one
//! definition. Constants are also drawn from [`runtime::tool::iii_tool`]
//! where possible to keep docs/install URLs in a single place.
//!
//! See `crates/motia-tools/src/main.rs` for the parallel `MotiaConfig` impl
//! (which lives in its own binary because that binary is published
//! separately as `motia-tools`).
//!
//! [`ProductConfig`]: crate::ProductConfig

use crate::product::ProductConfig;

/// Product-config impl for iii.
#[derive(Debug, Clone, Default)]
pub struct IiiConfig;

impl ProductConfig for IiiConfig {
    fn name(&self) -> &'static str {
        "iii"
    }

    fn display_name(&self) -> &'static str {
        "iii"
    }

    fn default_template_url(&self) -> &'static str {
        "https://github.com/iii-hq/templates.git"
    }

    fn template_url_env(&self) -> &'static str {
        "III_TEMPLATE_URL"
    }

    fn requires_iii(&self) -> bool {
        true
    }

    fn docs_url(&self) -> &'static str {
        // Source of truth lives in `runtime::tool::iii_tool()`. We
        // re-state the URL here because the trait demands `&'static str`
        // and `iii_tool().config().docs_url` is an opaque accessor; if
        // the source of truth moves, a search for `iii.dev/docs` will
        // surface every site that needs to be updated.
        "https://iii.dev/docs"
    }

    fn cli_description(&self) -> &'static str {
        "CLI for scaffolding iii projects"
    }

    fn upgrade_command(&self) -> &'static str {
        "iii update"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iii_config_defaults_match_runtime_tool() {
        let cfg = IiiConfig;
        assert_eq!(cfg.name(), "iii");
        assert_eq!(cfg.display_name(), "iii");
        assert_eq!(
            cfg.docs_url(),
            crate::runtime::tool::iii_tool().config().docs_url
        );
        assert!(cfg.requires_iii());
    }
}
