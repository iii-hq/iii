use serde::{Deserialize, Serialize};

use crate::modules::core_module::AdapterEntry;

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct StateModuleConfig {
    #[serde(default)]
    pub adapter: Option<AdapterEntry>,
}

impl Default for StateModuleConfig {
    fn default() -> Self {
        Self { adapter: None }
    }
}
