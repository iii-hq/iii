use std::{collections::HashMap, pin::Pin, sync::Arc};

use dashmap::DashMap;
use futures::Future;
use serde_json::Value;
use tokio::sync::oneshot::{self, error::RecvError};
use uuid::Uuid;

use crate::{function::Function, protocol::ErrorBody};

type InvocationFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Option<Value>, ErrorBody>> + Send + 'a>>;

type Invocations = Arc<DashMap<Uuid, oneshot::Sender<Result<Option<Value>, ErrorBody>>>>;

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
    ) -> InvocationFuture<'a>;
}

#[derive(Default)]
pub struct NonWorkerInvocations {
    invocations: Invocations,
}
impl NonWorkerInvocations {
    pub fn new() -> Self {
        Self {
            invocations: Arc::new(DashMap::new()),
        }
    }

    pub fn remove(
        &self,
        invocation_id: &Uuid,
    ) -> Option<oneshot::Sender<Result<Option<Value>, ErrorBody>>> {
        self.invocations
            .remove(invocation_id)
            .map(|(_, sender)| sender)
    }

    pub async fn handle_invocation(
        &self,
        body: Value,
        function_handler: Function,
        path_parameters: HashMap<String, String>,
        query_parameters: HashMap<String, String>,
    ) -> Result<Result<Option<Value>, ErrorBody>, RecvError> {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let invocation_id = uuid::Uuid::new_v4();
        self.invocations.insert(invocation_id, sender);
        let _ = function_handler
            .call_handler(Some(invocation_id), body)
            .await;
        receiver.await
    }
}
