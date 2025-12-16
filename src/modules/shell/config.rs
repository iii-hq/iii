use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExecConfig {
    pub watch: Option<Vec<String>>,
    pub exec: Vec<String>,
}
