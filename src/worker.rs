use crate::function::FunctionHandler;
use crate::invocation::{Invocation, InvocationHandler};
use crate::protocol::*;
use crate::trigger::{Trigger, TriggerRegistrator};

use axum::extract::ws::Message as WsMessage;
use dashmap::DashMap;
use futures::Future;
use serde_json::Value;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::mpsc;
use uuid::Uuid;

#[derive(Debug)]
pub enum Outbound {
    Protocol(Message),
    Raw(WsMessage),
}

#[derive(Clone)]
pub struct Worker {
    pub id: Uuid,
    pub channel: mpsc::Sender<Outbound>,
    pub invocations: Arc<DashMap<Uuid, Invocation>>,
}

impl TriggerRegistrator for Worker {
    fn register_trigger<'a>(
        &'a self,
        trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + 'a>> {
        let sender = self.channel.clone();

        Box::pin(async move {
            sender
                .send(Outbound::Protocol(Message::RegisterTrigger {
                    id: trigger.id,
                    trigger_type: trigger.trigger_type,
                    function_path: trigger.function_path,
                    config: trigger.config,
                }))
                .await
                .map_err(|err| {
                    anyhow::anyhow!(
                        "failed to send register trigger message through worker channel: {}",
                        err
                    )
                })?;

            Ok(())
        })
    }

    fn unregister_trigger<'a>(
        &'a self,
        trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + 'a>> {
        let sender = self.channel.clone();

        Box::pin(async move {
            sender
                .send(Outbound::Protocol(Message::UnregisterTrigger {
                    id: trigger.id,
                    trigger_type: trigger.trigger_type,
                }))
                .await
                .map_err(|err| {
                    anyhow::anyhow!(
                        "failed to send unregister trigger message through worker channel: {}",
                        err
                    )
                })?;

            Ok(())
        })
    }
}

impl FunctionHandler for Worker {
    fn handle_function<'a>(
        &'a self,
        invocation_id: Option<Uuid>,
        function_path: String,
        input: Value,
    ) -> Pin<Box<dyn Future<Output = Result<Option<Value>, ErrorBody>> + Send + 'a>> {
        Box::pin(async move {
            self.channel
                .send(Outbound::Protocol(Message::InvokeFunction {
                    invocation_id,
                    function_path,
                    data: input,
                }))
                .await
                .map_err(|err| ErrorBody {
                    code: "channel_send_failed".into(),
                    message: err.to_string(),
                })?;
            Ok(None)
        })
    }
}

impl InvocationHandler for Worker {
    fn handle_invocation_result<'a>(
        &'a self,
        invocation: Invocation,
        result: Option<Value>,
        error: Option<ErrorBody>,
    ) -> Pin<Box<dyn Future<Output = Result<Option<Value>, ErrorBody>> + Send + 'a>> {
        Box::pin(async move {
            self.channel
                .send(Outbound::Protocol(Message::InvocationResult {
                    invocation_id: invocation.invocation_id,
                    function_path: invocation.function_path,
                    result: result.clone(),
                    error: error.clone(),
                }))
                .await
                .map_err(|err| ErrorBody {
                    code: "channel_send_failed".into(),
                    message: err.to_string(),
                })?;

            Ok(None)
        })
    }
}
