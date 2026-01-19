use serde::{Deserialize, Serialize};

use crate::modules::module::AdapterEntry;

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct CronModuleConfig {
    #[serde(default)]
    pub adapter: Option<AdapterEntry>,
}
