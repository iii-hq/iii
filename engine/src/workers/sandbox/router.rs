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
    sync::{Arc, Mutex, PoisonError, Weak},
    time::Duration,
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
    function::{FunctionHandler, FunctionResult, HandlerFn},
    protocol::ErrorBody,
    workers::traits::Worker,
};

const MISSING_OP_GRACE: Duration = Duration::from_millis(250);

#[derive(Clone)]
pub struct SandboxRouter {
    engine: Arc<Engine>,
    binding: Option<SandboxRouterConfig>,
    /// The `adapter::info` registration that last reported our major. A
    /// restarted or swapped adapter registers a new one, which is checked again.
    checked_info: Arc<Mutex<Option<Weak<HandlerFn>>>>,
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

        if let Err(e) = self.check_contract(b).await {
            return FunctionResult::Failure(e);
        }

        let target = adapter_function_id(&b.adapter, &function_id);
        let registered = |id: &str| {
            self.engine
                .functions
                .with_function(&b.namespace, id, |f| f.is_some())
        };
        if !registered(&target) {
            // A (re)connecting adapter adds and drops its ids one at a time in no
            // fixed order, so a missing op gets one grace period before S304.
            tokio::time::sleep(MISSING_OP_GRACE).await;
            if !registered(&target) {
                // A connected adapter still has `adapter::info`; then only the op is missing.
                if registered(&adapter_function_id(&b.adapter, ADAPTER_INFO_SUFFIX)) {
                    return FunctionResult::Failure(sandbox_error(
                        SandboxErrorCode::S304,
                        &format!("adapter {} does not implement {function_id}", b.adapter),
                    ));
                }
                return FunctionResult::Failure(not_connected(b));
            }
        }

        // The op was registered, so `function_not_found` is the adapter's own
        // error unless the op dropped since (a disconnect racing this call).
        match self
            .engine
            .call_value_with_metadata_ns(&b.namespace, &target, input, metadata)
            .await
        {
            Ok(v) => FunctionResult::Success(v),
            Err(e) if e.code == "function_not_found" && !registered(&target) => {
                FunctionResult::Failure(not_connected(b))
            }
            Err(e) => FunctionResult::Failure(e),
        }
    }

    async fn check_contract(&self, b: &SandboxRouterConfig) -> Result<(), ErrorBody> {
        let registration = self
            .engine
            .functions
            .get_handler(
                &b.namespace,
                &adapter_function_id(&b.adapter, ADAPTER_INFO_SUFFIX),
            )
            .ok_or_else(|| not_connected(b))?;
        let checked = self
            .checked_info
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .is_some_and(|w| std::ptr::addr_eq(w.as_ptr(), Arc::as_ptr(&registration)));
        if checked {
            return Ok(());
        }

        let info = self.adapter_info(b).await?;
        if info.contract.major != CONTRACT_MAJOR {
            return Err(sandbox_error(
                SandboxErrorCode::S302,
                &format!(
                    "adapter {adapter} implements sandbox contract {}.{}, but this engine needs major {CONTRACT_MAJOR}; install a {adapter} release whose major version is {CONTRACT_MAJOR}",
                    info.contract.major,
                    info.contract.minor,
                    adapter = b.adapter,
                ),
            ));
        }
        // A `Weak` keeps a later registration from reusing this one's address.
        *self
            .checked_info
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(Arc::downgrade(&registration));
        Ok(())
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
            .map_err(|_| not_connected(b))?;
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
            checked_info: Arc::new(Mutex::new(None)),
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
