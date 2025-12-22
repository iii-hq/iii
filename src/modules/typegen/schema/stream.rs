use std::path::PathBuf;

use super::ir::SchemaNode;

#[derive(Debug, Clone)]
pub struct StreamDefinition {
    pub name: String,
    pub file_path: PathBuf,
    pub schema: SchemaNode,
}


