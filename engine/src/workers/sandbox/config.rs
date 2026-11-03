// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! The `sandbox:` block of the engine config: which adapter worker the
//! `sandbox::*` router forwards to, and the namespace that worker runs in.

use serde::{Deserialize, Serialize};

fn default_namespace() -> String {
    "default".to_string()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxRouterConfig {
    pub adapter: String,
    #[serde(default = "default_namespace")]
    pub namespace: String,
}

impl SandboxRouterConfig {
    /// Rejects a binding the router could never resolve, so startup and reload
    /// fail before any worker changes.
    pub fn validate(&self) -> anyhow::Result<()> {
        if !iii_sandbox_contract::is_valid_adapter_name(&self.adapter) {
            anyhow::bail!(
                "[INVALID_SANDBOX_CONFIG] sandbox.adapter '{}' must match [a-z0-9-]+ and must not be 'sandbox'",
                self.adapter
            );
        }
        let invalid = if self.namespace.is_empty() {
            Err("it is blank")
        } else {
            iii_compose::namespace::check(&self.namespace)
        };
        if let Err(reason) = invalid {
            anyhow::bail!(
                "[INVALID_SANDBOX_CONFIG] sandbox.namespace '{}': {reason}",
                self.namespace
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespace_defaults_to_default() {
        let c: SandboxRouterConfig = serde_yaml::from_str("adapter: sandbox-libkrun").unwrap();
        assert_eq!(c.namespace, "default");
        c.validate().unwrap();
    }

    #[test]
    fn unknown_keys_are_rejected() {
        assert!(
            serde_yaml::from_str::<SandboxRouterConfig>("adapter: x\nimage_allowlist: []").is_err()
        );
    }

    #[test]
    fn validate_rejects_bad_adapter_names() {
        for bad in [
            "",
            "Sandbox-Libkrun",
            "sandbox",
            "sandbox_libkrun",
            "fake::x",
        ] {
            let err = SandboxRouterConfig {
                adapter: bad.into(),
                namespace: "default".into(),
            }
            .validate()
            .unwrap_err()
            .to_string();
            assert_eq!(
                err,
                format!(
                    "[INVALID_SANDBOX_CONFIG] sandbox.adapter '{bad}' must match [a-z0-9-]+ and must not be 'sandbox'"
                )
            );
        }
    }

    #[test]
    fn validate_rejects_bad_namespaces() {
        for (bad, reason) in [
            (
                "My-Proj",
                "namespaces are lowercase, and are not lowercased for you",
            ),
            ("my proj", "it may hold only a-z, 0-9, '-' and '_'"),
            ("", "it is blank"),
        ] {
            let err = SandboxRouterConfig {
                adapter: "fake".into(),
                namespace: bad.into(),
            }
            .validate()
            .unwrap_err()
            .to_string();
            assert_eq!(
                err,
                format!("[INVALID_SANDBOX_CONFIG] sandbox.namespace '{bad}': {reason}")
            );
        }
    }
}
