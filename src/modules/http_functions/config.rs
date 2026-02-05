// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use serde::{Deserialize, Serialize};

use crate::{
    config::SecurityConfig, invocation::http_function::HttpFunctionConfig,
    triggers::http_trigger::HttpTriggerConfig,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[derive(Default)]
pub struct HttpFunctionsConfig {
    #[serde(default)]
    pub functions: Vec<HttpFunctionConfig>,
    #[serde(default)]
    pub triggers: Vec<HttpTriggerConfig>,
    #[serde(default)]
    pub security: SecurityConfig,
}

