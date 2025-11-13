use std::pin::Pin;

use futures::Future;
use serde_json::Value;
use uuid::Uuid;

use crate::protocol::ErrorBody;

pub struct Invocation {
    pub invocation_id: Uuid,
    pub function_path: String,
}

pub trait InvocationHandler {
    fn handle_invocation_result<'a>(
        &'a self,
        invocation: Invocation,
        result: Option<Value>,
        error: Option<ErrorBody>,
    ) -> Pin<Box<dyn Future<Output = Result<Option<Value>, ErrorBody>> + Send + 'a>>;
}
