use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, Default)]
pub struct LoggerModuleConfig {
    #[serde(default)]
    pub level: Option<String>,

    #[serde(default)]
    pub format: Option<String>,
}
