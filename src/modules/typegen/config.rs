use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeGenConfig {
    pub watch: Vec<String>,
    pub output: String,
    #[serde(default = "default_language")]
    pub language: String,
}

fn default_language() -> String {
    "typescript".to_string()
}

impl Default for TypeGenConfig {
    fn default() -> Self {
        Self {
            watch: vec!["src/**/*.step.ts".to_string()],
            output: "types.d.ts".to_string(),
            language: default_language(),
        }
    }
}


