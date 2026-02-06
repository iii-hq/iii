use async_trait::async_trait;
use serde_json::Value;
use uuid::Uuid;

use crate::{function::Function, protocol::ErrorBody};

#[async_trait]
pub trait Invoker: Send + Sync {
    fn method_type(&self) -> &'static str;

    async fn invoke(
        &self,
        function: &Function,
        invocation_id: Uuid,
        data: Value,
        caller_function: Option<&str>,
        trace_id: Option<&str>,
    ) -> Result<Option<Value>, ErrorBody>;
}
