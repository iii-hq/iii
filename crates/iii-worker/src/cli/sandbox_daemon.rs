// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

//! CLI entrypoint for `iii-worker sandbox-daemon`. Loads config and
//! delegates to `sandbox_daemon::run`.

use crate::cli::app::SandboxDaemonArgs;
use crate::sandbox_daemon;
use crate::sandbox_daemon::config::{self, SandboxConfig};

pub async fn run(args: SandboxDaemonArgs) -> i32 {
    // Fail-closed: any config error aborts startup.
    let cfg: SandboxConfig = match config::load_config(&args.config) {
        Ok(c) => {
            tracing::info!(
                auto_install = c.auto_install,
                allowlist_size = c.image_allowlist.len(),
                custom_images = c.custom_images.len(),
                max_concurrent = c.max_concurrent_sandboxes,
                default_idle_timeout_secs = c.default_idle_timeout_secs,
                "loaded sandbox-daemon config from {}",
                args.config
            );
            c
        }
        Err(e) => {
            tracing::error!(error = %e, path = %args.config,
                "failed to load sandbox-daemon config; aborting (fail-closed)");
            return 2;
        }
    };

    match sandbox_daemon::run(cfg, &args.engine).await {
        Ok(()) => 0,
        Err(e) => {
            tracing::error!(error = %e, "sandbox-daemon exited with error");
            1
        }
    }
}
