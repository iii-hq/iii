// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// See LICENSE and PATENTS files for details.

use serde::Deserialize;

use crate::modules::module::AdapterEntry;

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct LoggerModuleConfig {
    #[serde(default)]
    pub level: Option<String>,

    #[serde(default)]
    pub format: Option<String>,
    pub adapter: Option<AdapterEntry>,
}
