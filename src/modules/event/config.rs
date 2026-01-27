// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// See LICENSE and PATENTS files for details.

use serde::Deserialize;

use crate::modules::module::AdapterEntry;
#[allow(dead_code)] // this is used as default value
fn default_redis_url() -> String {
    "redis://localhost:6379".to_string()
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct EventModuleConfig {
    #[serde(default)]
    pub adapter: Option<AdapterEntry>,
}
