pub mod typescript;

use crate::modules::typegen::schema::{StepDefinition, StreamDefinition};

pub trait TypeGenerator: Send + Sync {
    fn generate(&self, steps: &[StepDefinition], streams: &[StreamDefinition]) -> String;
}


