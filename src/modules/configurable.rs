use std::sync::Arc;

use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::engine::Engine;

/// Trait for modules that can be configured via YAML/JSON
pub trait Configurable: Sized {
    /// The specific configuration type for this module
    /// **IMPORTANT**: Must use `#[serde(deny_unknown_fields)]`
    type Config: DeserializeOwned + Default;

    /// Creates the module with the provided configuration
    fn with_config(engine: Arc<Engine>, config: Self::Config) -> Self;

    /// Creates the module from an optional Value
    /// If None or parse error, uses Config::default()
    fn from_value(engine: Arc<Engine>, value: Option<Value>) -> Result<Self, serde_json::Error> {
        let config = match value {
            Some(v) => serde_json::from_value(v)?,
            None => Self::Config::default(),
        };

        Ok(Self::with_config(engine, config))
    }

    /// Creates the module with default configuration
    fn new(engine: Arc<Engine>) -> Self {
        Self::with_config(engine, Self::Config::default())
    }
}
