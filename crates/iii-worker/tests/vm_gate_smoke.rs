// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Smoke test: proves the integration-vm feature gate compiles and activates.
//! Real VM tests are added in Phase 5.

#![cfg(feature = "integration-vm")]

#[test]
fn vm_feature_gate_active() {
    assert!(
        cfg!(feature = "integration-vm"),
        "integration-vm feature should be active"
    );
}
