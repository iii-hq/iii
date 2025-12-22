use std::collections::HashMap;
use std::path::PathBuf;

use super::ir::SchemaNode;

#[derive(Debug, Clone, PartialEq)]
pub enum StepType {
    Api {
        method: String,
        path: String,
    },
    Event,
    Cron {
        expression: String,
    },
}

#[derive(Debug, Clone)]
pub struct StepDefinition {
    pub name: String,
    pub step_type: StepType,
    pub file_path: PathBuf,
    pub body_schema: Option<SchemaNode>,
    pub response_schemas: HashMap<u16, SchemaNode>,
    pub emits: Vec<String>,
    pub subscribes: Vec<String>,
}


