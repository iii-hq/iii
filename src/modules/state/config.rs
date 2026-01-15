use serde::{Deserialize, Serialize};

use crate::modules::core_module::AdapterEntry;

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(deny_unknown_fields)]
pub struct StateModuleConfig {
    #[serde(default)]
    pub adapter: Option<AdapterEntry>,
}
