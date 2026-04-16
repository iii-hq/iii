//! Generic tool management for CLI tools
//!
//! Provides a reusable abstraction for checking and installing CLI tools
//! like iii, or any other tool that can be installed via a shell script.

use anyhow::Result;
use colored::Colorize;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command as TokioCommand;
use tokio::time::timeout;

/// Timeout for installation (30 seconds)
const INSTALL_TIMEOUT: Duration = Duration::from_secs(30);

/// Configuration for a CLI tool
#[derive(Debug, Clone)]
pub struct ToolConfig {
    /// Name of the tool binary (e.g., "iii")
    pub name: &'static str,
    /// Display name for user-facing messages
    pub display_name: &'static str,
    /// URL to the install script
    pub install_script_url: &'static str,
    /// URL to the documentation
    pub docs_url: &'static str,
}

/// Manager for checking and installing CLI tools
pub struct ToolManager {
    config: ToolConfig,
}

impl ToolManager {
    /// Create a new tool manager with the given configuration
    pub fn new(config: ToolConfig) -> Self {
        Self { config }
    }

    /// Get the tool configuration
    pub fn config(&self) -> &ToolConfig {
        &self.config
    }

    /// Get the install command string
    pub fn install_command(&self) -> String {
        format!("curl -fsSL {} | sh", self.config.install_script_url)
    }

    /// Check if the tool is installed and available in PATH
    pub fn is_installed(&self) -> bool {
        std::process::Command::new("which")
            .arg(self.config.name)
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    /// Get the installed tool version (if available)
    pub fn get_version(&self) -> Option<String> {
        std::process::Command::new(self.config.name)
            .arg("--version")
            .output()
            .ok()
            .and_then(|output| {
                if output.status.success() {
                    String::from_utf8(output.stdout)
                        .ok()
                        .map(|s| s.trim().to_string())
                } else {
                    None
                }
            })
    }

    /// Install the tool using its official install script
    /// Shows the command being executed and streams output
    pub async fn install(&self) -> Result<()> {
        let cmd = self.install_command();
        println!();
        println!("{} {}", "Running:".dimmed(), cmd.yellow());
        println!();

        // Create the command
        let mut child = TokioCommand::new("sh")
            .arg("-c")
            .arg(&cmd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        // Get stdout and stderr
        let stdout = child.stdout.take().expect("Failed to capture stdout");
        let stderr = child.stderr.take().expect("Failed to capture stderr");

        let mut stdout_reader = BufReader::new(stdout).lines();
        let mut stderr_reader = BufReader::new(stderr).lines();

        // Stream output with timeout
        let output_task = async {
            loop {
                tokio::select! {
                    line = stdout_reader.next_line() => {
                        match line {
                            Ok(Some(line)) => println!("  {}", line),
                            Ok(None) => break,
                            Err(e) => {
                                eprintln!("{} {}", "Error reading stdout:".red(), e);
                                break;
                            }
                        }
                    }
                    line = stderr_reader.next_line() => {
                        match line {
                            Ok(Some(line)) => eprintln!("  {}", line.yellow()),
                            Ok(None) => {}
                            Err(e) => {
                                eprintln!("{} {}", "Error reading stderr:".red(), e);
                            }
                        }
                    }
                }
            }
        };

        // Wait for output with timeout
        match timeout(INSTALL_TIMEOUT, output_task).await {
            Ok(_) => {}
            Err(_) => {
                // Kill the process on timeout
                let _ = child.kill().await;
                println!();
                anyhow::bail!(
                    "Installation timed out after {} seconds.\n\
                     The server may be unreachable. Please try again later or install manually:\n\
                     {}",
                    INSTALL_TIMEOUT.as_secs(),
                    cmd
                );
            }
        }

        // Wait for process to complete with timeout
        match timeout(Duration::from_secs(5), child.wait()).await {
            Ok(Ok(status)) => {
                println!();
                if status.success() {
                    Ok(())
                } else {
                    anyhow::bail!(
                        "Installation failed with exit code: {}\n\
                         Please try installing manually: {}",
                        status.code().unwrap_or(-1),
                        cmd
                    );
                }
            }
            Ok(Err(e)) => {
                anyhow::bail!("Failed to wait for installer: {}", e);
            }
            Err(_) => {
                let _ = child.kill().await;
                anyhow::bail!(
                    "Installation process hung. Please try installing manually:\n{}",
                    cmd
                );
            }
        }
    }

    /// Open the tool's documentation in the default browser
    pub fn open_docs(&self) -> Result<()> {
        println!(
            "{}",
            format!("Opening {} documentation in your browser...", self.config.display_name).cyan()
        );
        open::that(self.config.docs_url)?;
        Ok(())
    }
}

/// Pre-configured tool manager for iii
pub fn iii_tool() -> ToolManager {
    ToolManager::new(ToolConfig {
        name: "iii",
        display_name: "iii",
        install_script_url: "https://install.iii.dev/latest.sh",
        docs_url: "https://iii.dev/docs",
    })
}
