// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

//! Black-box test: the engine resolves the well-known worker name
//! `iii-sandbox` to an `iii-worker sandbox-daemon ...` invocation
//! without any iii.toml / iii_workers/ setup.

use serial_test::serial;

#[test]
#[serial]
fn iii_sandbox_resolves_to_path_iii_worker() {
    let dir = tempfile::tempdir().unwrap();
    let fake = dir.path().join("iii-worker");
    std::fs::write(&fake, "#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    let orig = std::env::var_os("PATH");
    // SAFETY: test is #[serial]; no other test mutates PATH concurrently.
    unsafe {
        std::env::set_var("PATH", dir.path());
    }

    let info =
        iii::workers::external::resolve_external_module_in(dir.path(), "workers::iii_sandbox");

    // SAFETY: same — restoring original PATH.
    unsafe {
        if let Some(v) = orig {
            std::env::set_var("PATH", v);
        } else {
            std::env::remove_var("PATH");
        }
    }

    let info = info.expect("iii-sandbox must resolve via KNOWN_EXTERNAL");
    assert_eq!(info.name, "iii-sandbox");
    assert_eq!(info.binary_path, fake);
    assert_eq!(info.extra_args, vec!["sandbox-daemon".to_string()]);
}

#[test]
#[serial]
fn iii_sandbox_returns_none_without_iii_worker_on_path() {
    let dir = tempfile::tempdir().unwrap();

    let orig_path = std::env::var_os("PATH");
    let orig_home = std::env::var_os("HOME");
    // SAFETY: #[serial] guarantees no parallel env mutation.
    unsafe {
        std::env::set_var("PATH", dir.path());
        std::env::set_var("HOME", dir.path());
    }

    let result =
        iii::workers::external::resolve_external_module_in(dir.path(), "workers::iii_sandbox");

    // SAFETY: restoring originals.
    unsafe {
        if let Some(v) = orig_path {
            std::env::set_var("PATH", v);
        } else {
            std::env::remove_var("PATH");
        }
        if let Some(v) = orig_home {
            std::env::set_var("HOME", v);
        } else {
            std::env::remove_var("HOME");
        }
    }

    assert!(
        result.is_none(),
        "expected None when iii-worker is not on PATH"
    );
}
