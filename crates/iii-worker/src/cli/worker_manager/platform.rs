// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Platform-specific utilities for VM execution (macOS entitlements, terminal management).

use anyhow::{Context, Result};

/// Ensure the binary has the required VM entitlements (macOS only).
#[cfg(target_os = "macos")]
pub fn ensure_macos_entitlements(binary: &std::path::Path) -> Result<()> {
    use std::process::Command;

    let output = Command::new("codesign")
        .args(["-d", "--entitlements", "-"])
        .arg(binary)
        .output()
        .context("failed to run codesign")?;

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    if combined.contains("com.apple.security.hypervisor")
        && combined.contains("com.apple.security.cs.disable-library-validation")
    {
        return Ok(());
    }

    let entitlements_dir = std::env::temp_dir();
    let plist_path = entitlements_dir.join("iii-vm-entitlements.plist");
    std::fs::write(
        &plist_path,
        concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
            "<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" ",
            "\"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n",
            "<plist version=\"1.0\">\n<dict>\n",
            "  <key>com.apple.security.hypervisor</key>\n  <true/>\n",
            "  <key>com.apple.security.cs.disable-library-validation</key>\n  <true/>\n",
            "</dict>\n</plist>\n",
        ),
    )?;

    let status = Command::new("codesign")
        .args(["--sign", "-", "--entitlements"])
        .arg(&plist_path)
        .arg("--force")
        .arg(binary)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .context("failed to run codesign")?;

    if !status.success() {
        anyhow::bail!("codesign exited with {}", status);
    }

    Ok(())
}

/// Keep the terminal's `ISIG` flag enabled so Ctrl+C generates SIGINT.
#[cfg(unix)]
pub async fn ensure_terminal_isig() {
    use nix::libc;
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        unsafe {
            let mut t: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(libc::STDERR_FILENO, &mut t) == 0 && (t.c_lflag & libc::ISIG == 0) {
                t.c_lflag |= libc::ISIG;
                libc::tcsetattr(libc::STDERR_FILENO, libc::TCSANOW, &t);
            }
        }
    }
}

#[cfg(not(unix))]
pub async fn ensure_terminal_isig() {
    std::future::pending::<()>().await;
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn test_ensure_terminal_isig_noop_on_non_tty() {
        // In CI / test environments stderr is typically not a TTY.
        // The function should not panic — it just loops forever.
        // We spawn it and immediately cancel to verify no panic on entry.
        let handle = tokio::spawn(super::ensure_terminal_isig());
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        handle.abort();
        let result = handle.await;
        assert!(result.is_err()); // JoinError::Cancelled is expected
    }
}
