// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! The `sandbox::*` router: registers every public sandbox id in `default` and
//! forwards each call, payload and metadata untouched, to the bound adapter's
//! `<adapter>::*` id in the adapter's namespace.

use std::{
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use futures::Future;
use iii_sandbox_contract::{
    ADAPTER_INFO_SUFFIX, AdapterInfo, AdapterInfoRequest, CONTRACT_MAJOR, ContractVersion,
    OPERATIONS, ROUTER_INFO_ID, RouterInfo, SandboxErrorCode, adapter_function_id, error_payload,
};
use serde_json::{Value, json};
use uuid::Uuid;

use super::config::SandboxRouterConfig;
use crate::{
    engine::{Engine, EngineTrait, RegisterFunctionRequest},
    function::{FunctionHandler, FunctionResult},
    protocol::ErrorBody,
    workers::traits::Worker,
};

#[derive(Clone)]
pub struct SandboxRouter {
    engine: Arc<Engine>,
    binding: Option<SandboxRouterConfig>,
    /// Set once the bound adapter's `adapter::info` reported our major; cleared
    /// on S301 or `invocation_stopped`.
    handshake_ok: Arc<AtomicBool>,
}

fn sandbox_error(code: SandboxErrorCode, message: &str) -> ErrorBody {
    ErrorBody {
        code: "invocation_failed".into(),
        message: error_payload(code, message, None, None).to_string(),
        stacktrace: None,
    }
}

fn not_connected(b: &SandboxRouterConfig) -> ErrorBody {
    sandbox_error(
        SandboxErrorCode::S301,
        &format!(
            "adapter {} not connected in namespace {}; compose::add iii-sandbox in that project installs it",
            b.adapter, b.namespace
        ),
    )
}

impl SandboxRouter {
    async fn route(
        &self,
        function_id: String,
        input: Value,
        metadata: Option<Value>,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        if function_id == ROUTER_INFO_ID {
            let info = match &self.binding {
                Some(b) => self.adapter_info(b).await.ok(),
                None => None,
            };
            let reply = RouterInfo {
                adapter: self.binding.as_ref().map(|b| b.adapter.clone()),
                namespace: self.binding.as_ref().map(|b| b.namespace.clone()),
                contract: ContractVersion::CURRENT,
                info,
            };
            return FunctionResult::Success(serde_json::to_value(reply).ok());
        }

        let Some(b) = &self.binding else {
            return FunctionResult::Failure(sandbox_error(
                SandboxErrorCode::S303,
                "no sandbox adapter configured: this engine is not managed by a Compose file; set sandbox.adapter in its engine config (Compose-managed engines default to sandbox-libkrun)",
            ));
        };

        if !self.handshake_ok.load(Ordering::Relaxed) {
            let info = match self.adapter_info(b).await {
                Ok(info) => info,
                Err(e) => return FunctionResult::Failure(e),
            };
            if info.contract.major != CONTRACT_MAJOR {
                return FunctionResult::Failure(sandbox_error(
                    SandboxErrorCode::S302,
                    &format!(
                        "adapter {adapter} implements sandbox contract {}.{}, but this engine needs major {CONTRACT_MAJOR}; install a {adapter} release whose major version is {CONTRACT_MAJOR}",
                        info.contract.major,
                        info.contract.minor,
                        adapter = b.adapter,
                    ),
                ));
            }
            self.handshake_ok.store(true, Ordering::Relaxed);
        }

        let target = adapter_function_id(&b.adapter, &function_id);
        match self
            .engine
            .call_value_with_metadata_ns(&b.namespace, &target, input, metadata)
            .await
        {
            Ok(v) => FunctionResult::Success(v),
            Err(e) if e.code == "function_not_found" => {
                // A connected adapter still has `adapter::info`; then only the op is missing.
                let info_id = adapter_function_id(&b.adapter, ADAPTER_INFO_SUFFIX);
                if self
                    .engine
                    .functions
                    .with_function(&b.namespace, &info_id, |f| f.is_some())
                {
                    return FunctionResult::Failure(sandbox_error(
                        SandboxErrorCode::S304,
                        &format!("adapter {} does not implement {function_id}", b.adapter),
                    ));
                }
                self.handshake_ok.store(false, Ordering::Relaxed);
                FunctionResult::Failure(not_connected(b))
            }
            Err(e) => {
                if e.code == "invocation_stopped" {
                    self.handshake_ok.store(false, Ordering::Relaxed);
                }
                FunctionResult::Failure(e)
            }
        }
    }

    async fn adapter_info(&self, b: &SandboxRouterConfig) -> Result<AdapterInfo, ErrorBody> {
        let reply = self
            .engine
            .call_value_with_metadata_ns(
                &b.namespace,
                &adapter_function_id(&b.adapter, ADAPTER_INFO_SUFFIX),
                json!({}),
                None,
            )
            .await
            .map_err(|_| {
                self.handshake_ok.store(false, Ordering::Relaxed);
                not_connected(b)
            })?;
        serde_json::from_value(reply.unwrap_or(Value::Null)).map_err(|error| {
            sandbox_error(
                SandboxErrorCode::S302,
                &format!(
                    "adapter {} returned an adapter::info reply this engine cannot read ({error}); this engine needs sandbox contract major {CONTRACT_MAJOR}",
                    b.adapter
                ),
            )
        })
    }
}

impl FunctionHandler for SandboxRouter {
    fn handle_function<'a>(
        &'a self,
        _invocation_id: Option<Uuid>,
        function_id: String,
        input: Value,
        metadata: Option<Value>,
    ) -> Pin<Box<dyn Future<Output = FunctionResult<Option<Value>, ErrorBody>> + Send + 'a>> {
        Box::pin(self.route(function_id, input, metadata))
    }
}

#[async_trait::async_trait]
impl Worker for SandboxRouter {
    fn name(&self) -> &'static str {
        "iii-sandbox"
    }

    async fn create(engine: Arc<Engine>, config: Option<Value>) -> anyhow::Result<Box<dyn Worker>> {
        let binding = config
            .map(serde_json::from_value::<SandboxRouterConfig>)
            .transpose()?;
        Ok(Box::new(SandboxRouter {
            engine,
            binding,
            handshake_ok: Arc::new(AtomicBool::new(false)),
        }))
    }

    async fn initialize(&self) -> anyhow::Result<()> {
        Ok(())
    }

    fn register_functions(&self, engine: Arc<Engine>) {
        for op in OPERATIONS {
            engine.register_function(
                RegisterFunctionRequest {
                    function_id: op.public_id(),
                    description: Some(op.description.into()),
                    request_format: Some((op.request_schema)()),
                    response_format: Some((op.response_schema)()),
                    metadata: None,
                },
                Box::new(self.clone()),
            );
        }
        engine.register_function(
            RegisterFunctionRequest {
                function_id: ROUTER_INFO_ID.into(),
                description: Some(
                    "Reports the bound sandbox adapter, its namespace and contract version".into(),
                ),
                request_format: serde_json::to_value(schemars::schema_for!(AdapterInfoRequest))
                    .ok(),
                response_format: serde_json::to_value(schemars::schema_for!(RouterInfo)).ok(),
                metadata: Some(json!({"internal": true})),
            },
            Box::new(self.clone()),
        );
    }
}
