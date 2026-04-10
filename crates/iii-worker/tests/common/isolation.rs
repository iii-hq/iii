// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::sync::Mutex;

/// Process-global lock serializing tests that mutate CWD.
pub static CWD_LOCK: Mutex<()> = Mutex::new(());

/// Run an async closure in a temp dir, restoring cwd afterward.
pub async fn in_temp_dir_async<F, Fut>(f: F)
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let _guard = CWD_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let original = std::env::current_dir().unwrap();
    std::env::set_current_dir(dir.path()).unwrap();
    f().await;
    std::env::set_current_dir(original).unwrap();
}

/// Run a closure in a temp dir, restoring cwd afterward.
pub fn in_temp_dir<F: FnOnce()>(f: F) {
    let _guard = CWD_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let original = std::env::current_dir().unwrap();
    std::env::set_current_dir(dir.path()).unwrap();
    f();
    std::env::set_current_dir(original).unwrap();
}
