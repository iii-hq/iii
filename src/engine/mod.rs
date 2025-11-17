use serde_json::Value;

use crate::function::FunctionHandler;
use crate::trigger::TriggerType;

pub trait EngineTrait {
    fn register_function(
        &self,
        function_path: &str,
        function: Box<dyn FunctionHandler + Send + Sync>,
    );

    fn invoke_function(&self, function_path: &str, input: Value);
    fn register_trigger_type(&self, trigger_type: TriggerType);
}
