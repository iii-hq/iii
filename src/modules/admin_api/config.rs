// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use serde::{Deserialize, Serialize};

const DEFAULT_ADMIN_PORT: u16 = 49135;
const DEFAULT_ADMIN_HOST: &str = "127.0.0.1";

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AdminApiConfig {
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_host")]
    pub host: String,
}

fn default_port() -> u16 {
    DEFAULT_ADMIN_PORT
}

fn default_host() -> String {
    DEFAULT_ADMIN_HOST.to_string()
}

impl Default for AdminApiConfig {
    fn default() -> Self {
        Self {
            port: DEFAULT_ADMIN_PORT,
            host: DEFAULT_ADMIN_HOST.to_string(),
        }
    }
}
