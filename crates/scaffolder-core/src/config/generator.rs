//! Configuration file utilities

/// Supported JavaScript runtimes in order of preference
const JS_RUNTIMES: &[(&str, &str)] = &[
    ("bun", "bun run --enable-source-maps index-production.js"),
    ("node", "node --enable-source-maps index-production.js"),
];

/// Detect the available JavaScript runtime
pub fn detect_js_runtime() -> &'static str {
    for (runtime, command) in JS_RUNTIMES {
        if std::process::Command::new(runtime)
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
        {
            return command;
        }
    }
    // Default to bun if nothing detected (will fail at runtime with helpful error)
    JS_RUNTIMES[0].1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_js_runtime_returns_valid_command() {
        let runtime = detect_js_runtime();
        assert!(runtime.contains("index-production.js"));
    }
}
