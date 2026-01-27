// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// See LICENSE and PATENTS files for details.

use std::sync::Arc;

use dashmap::DashMap;
use serde_json::Value;
use tokio::sync::oneshot::{self, error::RecvError};
use uuid::Uuid;

use crate::{
    function::{Function, FunctionResult},
    protocol::ErrorBody,
};

pub struct Invocation {
    pub id: Uuid,
    pub function_path: String,
    pub worker_id: Option<Uuid>,
    pub sender: oneshot::Sender<Result<Option<Value>, ErrorBody>>,
}

type Invocations = Arc<DashMap<Uuid, Invocation>>;

#[derive(Default)]
pub struct InvocationHandler {
    invocations: Invocations,
}
impl InvocationHandler {
    pub fn new() -> Self {
        Self {
            invocations: Arc::new(DashMap::new()),
        }
    }

    pub fn remove(&self, invocation_id: &Uuid) -> Option<Invocation> {
        self.invocations
            .remove(invocation_id)
            .map(|(_, sender)| sender)
    }

    pub fn halt_invocation(&self, invocation_id: &Uuid) {
        let invocation = self.remove(invocation_id);

        if let Some(invocation) = invocation {
            let _ = invocation.sender.send(Err(ErrorBody {
                code: "invocation_stopped".into(),
                message: "Invocation stopped".into(),
            }));
        }
    }

    pub async fn handle_invocation(
        &self,
        invocation_id: Option<Uuid>,
        worker_id: Option<Uuid>,
        function_path: String,
        body: Value,
        function_handler: Function,
    ) -> Result<Result<Option<Value>, ErrorBody>, RecvError> {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let invocation_id = invocation_id.unwrap_or(Uuid::new_v4());
        let invocation = Invocation {
            id: invocation_id,
            function_path: function_path.clone(),
            worker_id,
            sender,
        };

        let result = function_handler
            .call_handler(Some(invocation_id), body)
            .await;

        match result {
            FunctionResult::Success(result) => {
                tracing::debug!(invocation_id = %invocation_id, function_path = %function_path, "Function result: {:?}", result);
                let _ = invocation.sender.send(Ok(result));
            }
            FunctionResult::Failure(error) => {
                tracing::debug!(invocation_id = %invocation_id, function_path = %function_path, "Function error: {:?}", error);
                let _ = invocation.sender.send(Err(error));
            }
            FunctionResult::NoResult => {
                tracing::debug!(invocation_id = %invocation_id, function_path = %function_path, "Function no result");
                let _ = invocation.sender.send(Ok(None));
            }
            FunctionResult::Deferred => {
                tracing::debug!(invocation_id = %invocation_id, function_path = %function_path, "Function deferred");
                // we need to store the invocation because it's a worker invocation
                self.invocations.insert(invocation_id, invocation);
            }
        }

        receiver.await
    }
}
