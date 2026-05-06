// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use std::path::Path;

use super::error::ExecError;

/// Run a binary with the given arguments.
///
/// On Unix: Uses the POSIX process-replacement syscall to hand off the
/// current process entirely. This ensures the child binary fully owns the
/// terminal (critical for interactive tools like iii-tools with cliclack).
///
/// On Windows: Spawns the binary as a child process with inherited stdio and
/// returns its exit code.
///
/// IMPORTANT: All iii output (progress bars, update notifications) MUST be
/// flushed before calling this function.
pub fn run_binary(binary_path: &Path, args: &[String]) -> Result<i32, ExecError> {
    if !binary_path.exists() {
        return Err(ExecError::BinaryNotFound {
            path: binary_path.display().to_string(),
        });
    }

    // Flush all output before replacing the process
    flush_output();

    #[cfg(unix)]
    {
        run_binary_unix(binary_path, args)
    }

    #[cfg(windows)]
    {
        run_binary_windows(binary_path, args)
    }
}

/// Flush stdout and stderr to ensure all iii output is visible
/// before the child binary takes over.
fn flush_output() {
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
}

/// Unix: Replace the current process with the binary using the POSIX
/// process-replacement syscall.
///
/// This function never returns on success -- the process image is replaced
/// entirely. The child binary inherits the terminal, PID, and all file
/// descriptors. This is NOT shell invocation -- no injection risk.
#[cfg(unix)]
fn run_binary_unix(binary_path: &Path, args: &[String]) -> Result<i32, ExecError> {
    use std::os::unix::process::CommandExt;

    // .exec() replaces the current process image (only returns on error)
    let err = std::process::Command::new(binary_path).args(args).exec();

    Err(ExecError::SpawnFailed {
        binary: binary_path.display().to_string(),
        source: err,
    })
}

/// Windows: Spawn the binary as a child process with inherited stdio.
#[cfg(windows)]
fn run_binary_windows(binary_path: &Path, args: &[String]) -> Result<i32, ExecError> {
    let status = std::process::Command::new(binary_path)
        .args(args)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .map_err(|e| ExecError::SpawnFailed {
            binary: binary_path.display().to_string(),
            source: e,
        })?;

    Ok(status.code().unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_binary_not_found() {
        let result = run_binary(&PathBuf::from("/nonexistent/binary"), &[]);
        assert!(result.is_err());
        match result.unwrap_err() {
            ExecError::BinaryNotFound { path } => {
                assert!(path.contains("nonexistent"));
            }
            _ => panic!("Expected BinaryNotFound error"),
        }
    }

    #[test]
    fn test_flush_output_no_panic() {
        flush_output();
    }
}
