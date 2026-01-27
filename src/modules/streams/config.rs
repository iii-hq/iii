// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// See LICENSE and PATENTS files for details.

use serde::{Deserialize, Serialize};

use crate::modules::module::AdapterEntry;

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct StreamModuleConfig {
    #[serde(default = "default_port")]
    pub port: u16,

    #[serde(default = "default_host")]
    pub host: String,

    #[serde(default)]
    pub auth_function: Option<String>,

    #[serde(default)]
    pub adapter: Option<AdapterEntry>,
}

fn default_port() -> u16 {
    3112
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}

impl Default for StreamModuleConfig {
    fn default() -> Self {
        Self {
            port: default_port(),
            host: default_host(),
            adapter: None,
            auth_function: None,
        }
    }
}
