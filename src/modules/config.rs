use serde::Deserialize;
use serde_json::Value;

// =============================================================================
// Engine Config (root of YAML)
// =============================================================================

#[derive(Debug, Deserialize)]
pub struct EngineConfig {
    #[serde(default)]
    pub modules: Vec<ModuleEntry>,
}

#[derive(Debug, Deserialize)]
pub struct ModuleEntry {
    pub class: String,
    #[serde(default)]
    pub config: Option<Value>,
}
