use crate::protocol::*;
use serde_json::Value;
use std::pin::Pin;
use uuid::Uuid;

pub trait FunctionHandler {
    fn handle_function<'a>(
        &'a self,
        invocation_id: Option<Uuid>,
        function_path: String,
        input: Value,
    ) -> Pin<Box<dyn Future<Output = Result<Option<Value>, ErrorBody>> + Send + 'a>>;
}
