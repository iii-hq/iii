use serde::{Deserialize, Serialize, de};
use std::collections::HashMap;

/// A worker declaration.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum WorkerDef {
    Managed {
        image: String,
        #[serde(default)]
        env: HashMap<String, String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resources: Option<WorkerResources>,
    },
    Binary {
        version: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        config: Option<serde_json::Value>,
    },
}

impl<'de> Deserialize<'de> for WorkerDef {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let map: serde_json::Map<String, serde_json::Value> =
            serde_json::Map::deserialize(deserializer)
                .map_err(|_| de::Error::custom("expected a YAML mapping"))?;

        let type_val = map.get("type").and_then(|v| v.as_str());

        match type_val {
            Some("binary") => {
                let version = map
                    .get("version")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| de::Error::missing_field("version"))?
                    .to_string();
                let config = map.get("config").cloned();
                Ok(WorkerDef::Binary { version, config })
            }
            Some("managed") | None => {
                let image = map
                    .get("image")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| de::Error::missing_field("image"))?
                    .to_string();
                let env: HashMap<String, String> = map
                    .get("env")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();
                let resources: Option<WorkerResources> = map
                    .get("resources")
                    .and_then(|v| serde_json::from_value(v.clone()).ok());
                Ok(WorkerDef::Managed {
                    image,
                    env,
                    resources,
                })
            }
            Some(other) => Err(de::Error::unknown_variant(other, &["binary", "managed"])),
        }
    }
}

impl WorkerDef {
    pub fn is_binary(&self) -> bool {
        matches!(self, WorkerDef::Binary { .. })
    }
    pub fn is_managed(&self) -> bool {
        matches!(self, WorkerDef::Managed { .. })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerResources {
    pub cpus: Option<String>,
    pub memory: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_binary_returns_true_for_binary() {
        let def = WorkerDef::Binary {
            version: "1.0.0".to_string(),
            config: None,
        };
        assert!(def.is_binary());
        assert!(!def.is_managed());
    }

    #[test]
    fn is_managed_returns_true_for_managed() {
        let def = WorkerDef::Managed {
            image: "ghcr.io/iii-hq/test:latest".to_string(),
            env: HashMap::new(),
            resources: None,
        };
        assert!(def.is_managed());
        assert!(!def.is_binary());
    }

    #[test]
    fn deserialize_managed_with_explicit_type() {
        let json = serde_json::json!({
            "type": "managed",
            "image": "ghcr.io/iii-hq/test:latest",
            "env": { "FOO": "bar" }
        });
        let def: WorkerDef = serde_json::from_value(json).unwrap();
        match def {
            WorkerDef::Managed { image, env, .. } => {
                assert_eq!(image, "ghcr.io/iii-hq/test:latest");
                assert_eq!(env.get("FOO").unwrap(), "bar");
            }
            _ => panic!("expected Managed variant"),
        }
    }

    #[test]
    fn deserialize_binary_with_config() {
        let json = serde_json::json!({
            "type": "binary",
            "version": "1.2.3",
            "config": { "key": "value" }
        });
        let def: WorkerDef = serde_json::from_value(json).unwrap();
        match def {
            WorkerDef::Binary { version, config } => {
                assert_eq!(version, "1.2.3");
                assert_eq!(config.unwrap()["key"], "value");
            }
            _ => panic!("expected Binary variant"),
        }
    }

    #[test]
    fn deserialize_legacy_without_type_defaults_to_managed() {
        let json = serde_json::json!({
            "image": "ghcr.io/iii-hq/legacy:latest",
            "env": { "KEY": "value" }
        });
        let def: WorkerDef = serde_json::from_value(json).unwrap();
        match def {
            WorkerDef::Managed { image, env, .. } => {
                assert_eq!(image, "ghcr.io/iii-hq/legacy:latest");
                assert_eq!(env.get("KEY").unwrap(), "value");
            }
            _ => panic!("expected Managed variant for legacy input without type field"),
        }
    }
}
