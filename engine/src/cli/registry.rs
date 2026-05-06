// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use super::error::RegistryError;

#[derive(Debug, Clone)]
pub struct BinarySpec {
    pub name: &'static str,
    pub repo: &'static str,
    pub has_checksum: bool,
    pub supported_targets: &'static [&'static str],
    pub commands: &'static [CommandMapping],
    pub tag_prefix: Option<&'static str>,
}

#[derive(Debug, Clone)]
pub struct CommandMapping {
    /// The command name as exposed by iii (e.g., "console", "create")
    pub cli_command: &'static str,
    /// The subcommand to pass to the binary, or None for direct passthrough
    pub binary_subcommand: Option<&'static str>,
}

/// Specification for iii itself (the dispatcher).
/// Kept separate from REGISTRY because iii is not a dispatched binary.
pub static SELF_SPEC: BinarySpec = BinarySpec {
    name: "iii",
    repo: "iii-hq/iii",
    has_checksum: true,
    supported_targets: &[
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
        "x86_64-pc-windows-msvc",
        "aarch64-pc-windows-msvc",
        "x86_64-unknown-linux-gnu",
        "x86_64-unknown-linux-musl",
        "aarch64-unknown-linux-gnu",
    ],
    commands: &[],
    tag_prefix: Some("iii"),
};

pub static REGISTRY: &[BinarySpec] = &[
    BinarySpec {
        name: "iii-init",
        repo: "iii-hq/iii",
        has_checksum: true,
        supported_targets: &[
            "x86_64-unknown-linux-musl",
            "x86_64-unknown-linux-gnu",
            "aarch64-unknown-linux-musl",
            "aarch64-unknown-linux-gnu",
            "aarch64-apple-darwin",
            "x86_64-apple-darwin",
        ],
        commands: &[],
        tag_prefix: Some("iii"),
    },
    BinarySpec {
        name: "iii-console",
        repo: "iii-hq/iii",
        has_checksum: true,
        supported_targets: &[
            "aarch64-apple-darwin",
            "x86_64-apple-darwin",
            "x86_64-pc-windows-msvc",
            "aarch64-pc-windows-msvc",
            "x86_64-unknown-linux-gnu",
            "x86_64-unknown-linux-musl",
            "aarch64-unknown-linux-gnu",
        ],
        commands: &[CommandMapping {
            cli_command: "console",
            binary_subcommand: None,
        }],
        tag_prefix: Some("iii"),
    },
    BinarySpec {
        name: "iii-cloud",
        repo: "iii-hq/iii-cloud-cli",
        has_checksum: true,
        supported_targets: &[
            "aarch64-apple-darwin",
            "x86_64-apple-darwin",
            "x86_64-pc-windows-msvc",
            "aarch64-pc-windows-msvc",
            "x86_64-unknown-linux-gnu",
            "x86_64-unknown-linux-musl",
            "aarch64-unknown-linux-gnu",
        ],
        commands: &[CommandMapping {
            cli_command: "cloud",
            binary_subcommand: None,
        }],
        tag_prefix: None,
    },
    BinarySpec {
        name: "iii-worker",
        repo: "iii-hq/iii",
        has_checksum: true,
        supported_targets: &[
            "aarch64-apple-darwin",
            "x86_64-apple-darwin",
            "x86_64-unknown-linux-gnu",
            "x86_64-unknown-linux-musl",
            "aarch64-unknown-linux-gnu",
        ],
        commands: &[
            CommandMapping {
                cli_command: "worker",
                binary_subcommand: None,
            },
            CommandMapping {
                cli_command: "sandbox",
                binary_subcommand: Some("sandbox"),
            },
        ],
        tag_prefix: Some("iii"),
    },
];

pub fn resolve_command(
    command: &str,
) -> Result<(&'static BinarySpec, Option<&'static str>), RegistryError> {
    for spec in REGISTRY {
        for mapping in spec.commands {
            if mapping.cli_command == command {
                return Ok((spec, mapping.binary_subcommand));
            }
        }
    }
    Err(RegistryError::UnknownCommand {
        command: command.to_string(),
    })
}

/// Resolve a command name to its parent BinarySpec (for update resolution).
/// e.g., "create" resolves to iii-tools.
pub fn resolve_binary_for_update(command: &str) -> Result<&'static BinarySpec, RegistryError> {
    for spec in REGISTRY {
        if spec.name == command {
            return Ok(spec);
        }
    }
    for spec in REGISTRY {
        for mapping in spec.commands {
            if mapping.cli_command == command {
                return Ok(spec);
            }
        }
    }
    Err(RegistryError::UnknownCommand {
        command: command.to_string(),
    })
}

pub fn all_binaries() -> Vec<&'static BinarySpec> {
    REGISTRY.iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_console() {
        let (spec, sub) = resolve_command("console").unwrap();
        assert_eq!(spec.name, "iii-console");
        assert_eq!(spec.repo, "iii-hq/iii");
        assert!(sub.is_none());
    }

    #[test]
    fn test_create_no_longer_resolves() {
        // `create` was a managed-binary dispatch to iii-tools; the command
        // has moved to `iii project init --template`, and iii-tools is gone
        // from the registry.
        assert!(resolve_command("create").is_err());
    }

    #[test]
    fn test_resolve_cloud() {
        let (spec, sub) = resolve_command("cloud").unwrap();
        assert_eq!(spec.name, "iii-cloud");
        assert_eq!(spec.repo, "iii-hq/iii-cloud-cli");
        assert!(sub.is_none());
    }

    #[test]
    fn test_unknown_command() {
        assert!(resolve_command("foobar").is_err());
    }

    #[test]
    fn test_resolve_binary_for_update() {
        let spec = resolve_binary_for_update("iii-console").unwrap();
        assert_eq!(spec.name, "iii-console");
    }

    #[test]
    fn test_resolve_binary_for_update_sdk_not_in_registry() {
        // "sdk" is not a valid registry key; the update command must translate it
        assert!(resolve_binary_for_update("sdk").is_err());
    }

    #[test]
    fn test_console_has_checksum() {
        let (spec, _) = resolve_command("console").unwrap();
        assert!(spec.has_checksum);
    }

    #[test]
    fn test_self_spec_fields() {
        assert_eq!(SELF_SPEC.name, "iii");
        assert_eq!(SELF_SPEC.repo, "iii-hq/iii");
        assert!(SELF_SPEC.has_checksum);
        assert!(SELF_SPEC.commands.is_empty());
    }

    #[test]
    fn test_self_spec_supported_targets() {
        assert!(
            SELF_SPEC
                .supported_targets
                .contains(&"aarch64-apple-darwin")
        );
        assert!(SELF_SPEC.supported_targets.contains(&"x86_64-apple-darwin"));
        assert!(
            SELF_SPEC
                .supported_targets
                .contains(&"x86_64-unknown-linux-gnu")
        );
        assert!(
            SELF_SPEC
                .supported_targets
                .contains(&"x86_64-unknown-linux-musl")
        );
        assert!(
            SELF_SPEC
                .supported_targets
                .contains(&"aarch64-unknown-linux-gnu")
        );
        assert!(
            SELF_SPEC
                .supported_targets
                .contains(&"x86_64-pc-windows-msvc")
        );
        assert!(
            SELF_SPEC
                .supported_targets
                .contains(&"aarch64-pc-windows-msvc")
        );
        assert_eq!(SELF_SPEC.supported_targets.len(), 7);
    }

    #[test]
    fn test_self_spec_not_in_registry() {
        for spec in REGISTRY {
            assert_ne!(spec.name, "iii", "iii should not be in REGISTRY");
        }
    }

    #[test]
    fn test_self_spec_platform_support() {
        let result = crate::cli::platform::check_platform_support(&SELF_SPEC);
        assert!(result.is_ok());
    }
}
