// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// See LICENSE and PATENTS files for details.

use serde::{Deserialize, Serialize};

use crate::modules::module::AdapterEntry;

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(deny_unknown_fields)]
pub struct StateModuleConfig {
    #[serde(default)]
    pub adapter: Option<AdapterEntry>,
}
