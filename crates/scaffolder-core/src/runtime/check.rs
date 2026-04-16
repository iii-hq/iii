//! Runtime detection for Node.js, Bun, and Python

use anyhow::Result;
use std::fmt;
use std::process::Command;

/// Supported languages/runtimes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    TypeScript,
    JavaScript,
    Python,
    Rust,
}

impl Language {
    pub fn display_name(&self) -> &'static str {
        match self {
            Language::TypeScript => "TypeScript",
            Language::JavaScript => "JavaScript",
            Language::Python => "Python",
            Language::Rust => "Rust",
        }
    }
}

impl fmt::Display for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

/// Runtime detection result
#[derive(Debug, Clone)]
pub struct RuntimeInfo {
    pub name: &'static str,
    pub version: Option<String>,
    pub available: bool,
}

/// Check if Node.js is available
pub fn check_node() -> RuntimeInfo {
    let output = Command::new("node").arg("--version").output();

    match output {
        Ok(out) if out.status.success() => {
            let version = String::from_utf8_lossy(&out.stdout).trim().to_string();
            RuntimeInfo {
                name: "Node.js",
                version: Some(version),
                available: true,
            }
        }
        _ => RuntimeInfo {
            name: "Node.js",
            version: None,

            available: false,
        },
    }
}

/// Check if Bun is available
pub fn check_bun() -> RuntimeInfo {
    let output = Command::new("bun").arg("--version").output();

    match output {
        Ok(out) if out.status.success() => {
            let version = String::from_utf8_lossy(&out.stdout).trim().to_string();
            RuntimeInfo {
                name: "Bun",
                version: Some(version),
                available: true,
            }
        }
        _ => RuntimeInfo {
            name: "Bun",
            version: None,

            available: false,
        },
    }
}

/// Check if Python 3 is available
pub fn check_python() -> RuntimeInfo {
    let output = Command::new("python3").arg("--version").output();

    match output {
        Ok(out) if out.status.success() => {
            let version = String::from_utf8_lossy(&out.stdout).trim().to_string();
            RuntimeInfo {
                name: "Python 3",
                version: Some(version),
                available: true,
            }
        }
        _ => RuntimeInfo {
            name: "Python 3",
            version: None,

            available: false,
        },
    }
}

/// Check if Cargo/Rust is available
pub fn check_cargo() -> RuntimeInfo {
    let output = Command::new("cargo").arg("--version").output();

    match output {
        Ok(out) if out.status.success() => {
            let version = String::from_utf8_lossy(&out.stdout).trim().to_string();
            RuntimeInfo {
                name: "Cargo",
                version: Some(version),
                available: true,
            }
        }
        _ => RuntimeInfo {
            name: "Cargo",
            version: None,
            available: false,
        },
    }
}

/// Check runtimes with no advisory languages (strict mode - fail on any missing).
pub fn check_runtimes(languages: &[Language]) -> Result<Vec<RuntimeInfo>> {
    check_runtimes_with_advisory(languages, &[])
}

/// Check runtimes; languages in `advisory` get availability reported but don't cause failure.
pub fn check_runtimes_with_advisory(
    languages: &[Language],
    advisory: &[Language],
) -> Result<Vec<RuntimeInfo>> {
    let mut results = Vec::new();
    let mut missing = Vec::new();

    let is_advisory = |lang: &Language| advisory.contains(lang);

    // TypeScript and JavaScript both need Node.js or Bun
    let needs_js_runtime = languages
        .iter()
        .any(|l| matches!(l, Language::TypeScript | Language::JavaScript));

    if needs_js_runtime {
        let bun = check_bun();
        let node = check_node();
        let js_ts_langs: Vec<_> = languages
            .iter()
            .filter(|l| matches!(l, Language::TypeScript | Language::JavaScript))
            .collect();
        let js_advisory =
            !js_ts_langs.is_empty() && js_ts_langs.iter().all(|l| is_advisory(l));

        let any_js_available = bun.available || node.available;
        if bun.available {
            results.push(bun);
        }
        if node.available {
            results.push(node);
        }
        if !any_js_available {
            if js_advisory {
                results.push(RuntimeInfo {
                    name: "Node.js or Bun",
                    version: None,
                    available: false,
                });
            } else {
                missing.push("Node.js or Bun (install from https://nodejs.org or https://bun.sh)");
            }
        }
    }

    if languages.contains(&Language::Python) {
        let python = check_python();
        if python.available {
            results.push(python);
        } else if is_advisory(&Language::Python) {
            results.push(RuntimeInfo {
                name: "Python 3",
                version: None,
                available: false,
            });
        } else {
            missing.push("Python 3 (install from https://python.org)");
        }
    }

    if languages.contains(&Language::Rust) {
        let cargo = check_cargo();
        if cargo.available {
            results.push(cargo);
        } else if is_advisory(&Language::Rust) {
            results.push(RuntimeInfo {
                name: "Cargo",
                version: None,
                available: false,
            });
        } else {
            missing.push("Cargo/Rust (install from https://rustup.rs)");
        }
    }

    if !missing.is_empty() {
        anyhow::bail!(
            "Missing required runtimes:\n{}",
            missing
                .iter()
                .map(|m| format!("  - {}", m))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    Ok(results)
}
