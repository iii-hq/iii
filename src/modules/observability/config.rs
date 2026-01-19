use serde::Deserialize;

use crate::modules::module::AdapterEntry;

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct LoggerModuleConfig {
    #[serde(default)]
    pub level: Option<String>,

    #[serde(default)]
    pub format: Option<String>,
    pub adapter: Option<AdapterEntry>,
}
