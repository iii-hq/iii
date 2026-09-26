// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Linux process labels for the compose CLI and its managed engine.

/// The role of a process in a compose invocation.
#[derive(Clone, Copy)]
pub enum Role {
    Compose,
    Engine,
}

/// Full label used as argv[0], including namespaces longer than Linux comm.
pub fn command_name(role: Role, namespace: &str) -> String {
    let role = match role {
        Role::Compose => 'c',
        Role::Engine => 'e',
    };
    format!("iii:{role}:{namespace}")
}

/// Reads the label passed directly to a managed engine's argv[0]. Unlike an
/// environment variable, this label is not inherited by workers it starts.
pub fn engine_namespace(arg0: &str) -> Option<&str> {
    let namespace = arg0.strip_prefix("iii:e:")?;
    (!namespace.is_empty() && crate::namespace::check(namespace).is_ok()).then_some(namespace)
}

/// Labels the CLI process before it starts threads, telemetry, or children.
///
/// On Linux, re-execs the current binary with a full argv[0] if needed, keeping
/// the PID, arguments, environment, working directory, and standard streams.
/// This avoids overwriting Rust's argument storage or relocating environ.
/// The next invocation sets the main thread's comm before building the runtime.
/// Other platforms retain their existing process names.
pub fn set_current(role: Role, namespace: &str) -> std::io::Result<()> {
    if namespace.is_empty() || crate::namespace::check(namespace).is_err() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "invalid process namespace",
        ));
    }

    #[cfg(target_os = "linux")]
    {
        use std::{ffi::CString, os::unix::process::CommandExt, process::Command};

        let title = command_name(role, namespace);
        if std::env::args_os().next().as_deref() != Some(std::ffi::OsStr::new(&title)) {
            return Err(Command::new(std::env::current_exe()?)
                .arg0(&title)
                .args(std::env::args_os().skip(1))
                .exec());
        }

        let name = CString::new(short_name(role, namespace))?;
        nix::sys::prctl::set_name(&name).map_err(std::io::Error::from)?;
    }
    #[cfg(not(target_os = "linux"))]
    let _ = role;

    Ok(())
}

#[cfg(any(target_os = "linux", test))]
fn short_name(role: Role, namespace: &str) -> String {
    use sha2::{Digest, Sha256};

    let title = command_name(role, namespace);
    if title.len() <= 15 {
        return title;
    }

    // Validated namespaces are ASCII. Keep a readable prefix and a stable
    // suffix so names sharing their first characters do not simply truncate
    // to the same label. The full namespace remains available in argv[0].
    let digest = hex::encode(Sha256::digest(namespace.as_bytes()));
    format!(
        "{}{}~{}",
        command_name(role, ""),
        &namespace[..2],
        &digest[..6]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_names_show_the_role_and_complete_namespace() {
        assert_eq!(short_name(Role::Compose, "orders"), "iii:c:orders");
        assert_eq!(short_name(Role::Engine, "orders"), "iii:e:orders");
        assert_eq!(short_name(Role::Compose, "default"), "iii:c:default");
        assert_eq!(short_name(Role::Compose, "123456789"), "iii:c:123456789");
    }

    #[test]
    fn long_names_fit_comm_and_keep_shared_prefixes_distinct() {
        let orders = short_name(Role::Compose, "production-orders");
        let billing = short_name(Role::Compose, "production-billing");
        assert_eq!(orders.len(), 15);
        assert_eq!(billing.len(), 15);
        assert!(orders.starts_with("iii:c:pr~"));
        assert_ne!(orders, billing);
        assert_eq!(
            command_name(Role::Compose, "production-orders"),
            "iii:c:production-orders"
        );
    }

    #[test]
    fn only_valid_managed_engine_labels_supply_a_namespace() {
        assert_eq!(engine_namespace("iii:e:orders"), Some("orders"));
        for arg0 in ["iii", "/usr/bin/iii", "iii:c:orders", "iii:e:", "iii:e:BAD"] {
            assert_eq!(engine_namespace(arg0), None);
        }
    }
}
