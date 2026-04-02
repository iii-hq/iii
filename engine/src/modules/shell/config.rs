// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExecConfig {
    pub watch: Option<Vec<String>>,
    pub exec: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn exec_config_roundtrip() {
        let json = json!({"exec": ["node", "index.js"], "watch": ["src"]});
        let config: ExecConfig = serde_json::from_value(json).unwrap();
        assert_eq!(config.exec, vec!["node", "index.js"]);
        assert_eq!(config.watch, Some(vec!["src".to_string()]));
        let back = serde_json::to_value(&config).unwrap();
        assert_eq!(back["exec"][0], "node");
    }

    #[test]
    fn exec_config_optional_watch() {
        let json = json!({"exec": ["python", "main.py"]});
        let config: ExecConfig = serde_json::from_value(json).unwrap();
        assert!(config.watch.is_none());
    }
}
