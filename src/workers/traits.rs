// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// See LICENSE and PATENTS files for details.

use std::pin::Pin;

use futures::Future;
use serde_json::Value;
use uuid::Uuid;

use crate::{
    engine::Outbound,
    function::{FunctionHandler, FunctionResult},
    protocol::{ErrorBody, Message},
    trigger::{Trigger, TriggerRegistrator},
    workers::Worker,
};

impl TriggerRegistrator for Worker {
    fn register_trigger(
        &self,
        trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
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

    fn unregister_trigger(
        &self,
        trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
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
    ) -> Pin<Box<dyn Future<Output = FunctionResult<Option<Value>, ErrorBody>> + Send + 'a>> {
        Box::pin(async move {
            self.invocations
                .write()
                .await
                .insert(invocation_id.unwrap());

            let _ = self
                .channel
                .send(Outbound::Protocol(Message::InvokeFunction {
                    invocation_id,
                    function_path,
                    data: input,
                }))
                .await
                .map_err(|err| ErrorBody {
                    code: "channel_send_failed".into(),
                    message: err.to_string(),
                });

            FunctionResult::Deferred
        })
    }
}
