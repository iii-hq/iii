// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::http::{HeaderMap, Uri};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::engine::{Engine, EngineTrait};
use crate::modules::stream::utils::{headers_to_map, query_to_multi_map};
use crate::modules::worker::WorkerConfig;
use crate::protocol::ErrorBody;

use super::rbac_config::RbacConfig;

// ---------------------------------------------------------------------------
// Session
// ---------------------------------------------------------------------------

pub struct Session {
    pub engine: Arc<Engine>,
    pub config: Arc<WorkerConfig>,
    pub ip_address: String,
    pub session_id: Uuid,
    pub allowed_functions: Vec<String>,
    pub forbidden_functions: Vec<String>,
    pub allowed_trigger_types: Option<Vec<String>>,
    pub allow_trigger_type_registration: bool,
    pub context: Value,
}

pub(crate) struct AuthResult {
    allowed_functions: Vec<String>,
    forbidden_functions: Vec<String>,
    allowed_trigger_types: Option<Vec<String>>,
    allow_trigger_type_registration: bool,
    context: Value,
}

impl Session {
    pub(crate) async fn authenticate(
        engine: &Engine,
        rbac_config: Option<RbacConfig>,
        uri: Uri,
        headers: HeaderMap,
        ip_address: String,
    ) -> Result<AuthResult, ErrorBody> {
        let query_params = query_to_multi_map(uri.query());
        let headers = headers_to_map(&headers);
        let Some(auth_fn_id) = rbac_config.and_then(|c| c.auth_function_id) else {
            return Ok(AuthResult {
                allowed_functions: vec![],
                forbidden_functions: vec![],
                allowed_trigger_types: None,
                allow_trigger_type_registration: true,
                context: json!({}),
            });
        };

        let auth_input = json!({
            "headers": headers,
            "query_params": query_params,
            "ip_address": ip_address,
        });

        let result = engine
            .call(&auth_fn_id, auth_input)
            .await
            .map_err(|e| ErrorBody::new("AUTH_ERROR", e.message))?;

        let Some(value) = result else {
            return Err(ErrorBody::new(
                "AUTH_ERROR",
                "auth function returned no result",
            ));
        };

        let allowed_functions = value
            .get("allowed_functions")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let forbidden_functions = value
            .get("forbidden_functions")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let allowed_trigger_types = value
            .get("allowed_trigger_types")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect::<Vec<String>>()
            });

        let allow_trigger_type_registration = value
            .get("allow_trigger_type_registration")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let context = value.get("context").cloned().unwrap_or(json!({}));

        Ok(AuthResult {
            allowed_functions,
            forbidden_functions,
            allowed_trigger_types,
            allow_trigger_type_registration,
            context,
        })
    }
}

// ---------------------------------------------------------------------------
// Session lifecycle
// ---------------------------------------------------------------------------

pub async fn handle_session(
    addr: SocketAddr,
    engine: Arc<Engine>,
    config: Arc<WorkerConfig>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Session, ErrorBody> {
    let ip_address = addr.ip().to_string();
    let auth = Session::authenticate(
        &engine,
        config.rbac.clone(),
        uri,
        headers,
        ip_address.clone(),
    )
    .await?;

    Ok(Session {
        engine,
        config,
        ip_address,
        session_id: Uuid::new_v4(),
        allowed_functions: auth.allowed_functions,
        forbidden_functions: auth.forbidden_functions,
        allowed_trigger_types: auth.allowed_trigger_types,
        allow_trigger_type_registration: auth.allow_trigger_type_registration,
        context: auth.context,
    })
}
