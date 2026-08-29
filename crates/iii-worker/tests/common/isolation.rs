// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use std::sync::Mutex;

/// Process-global lock serializing tests that mutate CWD.
pub static CWD_LOCK: Mutex<()> = Mutex::new(());

struct RestoreProcessContext {
    cwd: std::path::PathBuf,
    home: Option<std::ffi::OsString>,
}

impl RestoreProcessContext {
    fn enter(path: &std::path::Path) -> Self {
        let restore = Self {
            cwd: std::env::current_dir().unwrap(),
            home: std::env::var_os("HOME"),
        };
        std::env::set_current_dir(path).unwrap();
        // SAFETY: every test in this integration-test process that mutates
        // CWD/HOME does so while holding CWD_LOCK.
        unsafe { std::env::set_var("HOME", path) };
        restore
    }
}

impl Drop for RestoreProcessContext {
    fn drop(&mut self) {
        std::env::set_current_dir(&self.cwd).expect("restore test working directory");
        // SAFETY: the CWD_LOCK guard outlives this restoration.
        unsafe {
            match &self.home {
                Some(home) => std::env::set_var("HOME", home),
                None => std::env::remove_var("HOME"),
            }
        }
    }
}

/// Run an async closure with both CWD and HOME isolated to a temp directory.
///
/// Uses `unwrap_or_else(into_inner)` for poison tolerance: when one test
/// panics the mutex is poisoned, and without tolerance every subsequent
/// sibling test also panics at the lock acquisition — turning a single
/// genuine failure into N cascading failures that hide the real cause.
pub async fn in_temp_dir_async<F, Fut>(f: F)
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let _guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let _restore = RestoreProcessContext::enter(dir.path());
    f().await;
}

/// Run a closure with both CWD and HOME isolated to a temp directory.
pub fn in_temp_dir<F: FnOnce()>(f: F) {
    let _guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let _restore = RestoreProcessContext::enter(dir.path());
    f();
}
