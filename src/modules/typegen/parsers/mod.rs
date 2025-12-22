pub mod typescript;
pub mod zod;

use std::path::Path;

use anyhow::Result;

use crate::modules::typegen::schema::{StepDefinition, StreamDefinition};

pub struct ParsedFile {
    pub steps: Vec<StepDefinition>,
    pub streams: Vec<StreamDefinition>,
}

pub trait LanguageParser: Send + Sync {
    fn supports_extension(&self, ext: &str) -> bool;
    fn parse_file(&self, path: &Path, content: &str) -> Result<ParsedFile>;
}


