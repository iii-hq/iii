use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevToolsConfig {
    
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    
    
    #[serde(default = "default_api_prefix")]
    pub api_prefix: String,
    
    
    #[serde(default = "default_metrics_enabled")]
    pub metrics_enabled: bool,
    
    
    #[serde(default = "default_metrics_interval")]
    pub metrics_interval: u64,
    
    
    #[serde(default = "default_state_stream")]
    pub state_stream: String,
    
    
    #[serde(default = "default_event_topic")]
    pub event_topic: String,
}

fn default_enabled() -> bool { true }
fn default_api_prefix() -> String { "_console".to_string() }
fn default_metrics_enabled() -> bool { true }
fn default_metrics_interval() -> u64 { 30 }
fn default_state_stream() -> String { "iii:devtools:state".to_string() }
fn default_event_topic() -> String { "iii:devtools:events".to_string() }

impl Default for DevToolsConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            api_prefix: default_api_prefix(),
            metrics_enabled: default_metrics_enabled(),
            metrics_interval: default_metrics_interval(),
            state_stream: default_state_stream(),
            event_topic: default_event_topic(),
        }
    }
}

