//! Template file copying with language filtering

use crate::runtime::check::Language;
use crate::templates::fetcher::TemplateFetcher;
use crate::templates::manifest::{FileLanguage, LanguageFiles, TemplateManifest};
use anyhow::{Context, Result};
use std::path::Path;
use tokio::fs;

/// Copy template files to the target directory, filtering by selected languages
pub async fn copy_template(
    fetcher: &mut TemplateFetcher,
    template_name: &str,
    manifest: &TemplateManifest,
    target_dir: &Path,
    selected_languages: &[Language],
    language_files: &LanguageFiles,
) -> Result<Vec<String>> {
    // Ensure target directory exists
    fs::create_dir_all(target_dir)
        .await
        .context("Failed to create target directory")?;

    let mut copied_files = Vec::new();

    for file_path in &manifest.files {
        // Check if this file should be included based on language selection
        if should_include_file(file_path, selected_languages, language_files) {
            // Ensure parent directories exist
            let target_path = target_dir.join(file_path);
            if let Some(parent) = target_path.parent() {
                fs::create_dir_all(parent)
                    .await
                    .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
            }

            // Fetch and write the file
            let content = fetcher.fetch_file_bytes(template_name, file_path).await?;
            fs::write(&target_path, &content)
                .await
                .with_context(|| format!("Failed to write file: {}", target_path.display()))?;

            copied_files.push(file_path.clone());
        }
    }

    Ok(copied_files)
}

/// Determine if a file should be included based on selected languages and language_files config
fn should_include_file(
    file_path: &str,
    selected_languages: &[Language],
    language_files: &LanguageFiles,
) -> bool {
    let has_typescript = selected_languages.contains(&Language::TypeScript);
    let has_javascript = selected_languages.contains(&Language::JavaScript);
    let has_python = selected_languages.contains(&Language::Python);
    let has_rust = selected_languages.contains(&Language::Rust);
    let has_js_or_ts = has_typescript || has_javascript;

    // Check if file matches any pattern from config
    match language_files.get_language_for_file(file_path) {
        Some(FileLanguage::Common) => true,
        Some(FileLanguage::Python) => has_python,
        Some(FileLanguage::TypeScript) => has_typescript,
        Some(FileLanguage::JavaScript) => has_javascript,
        Some(FileLanguage::Node) => has_js_or_ts,
        Some(FileLanguage::Rust) => has_rust,
        None => false, // File not in any list, exclude
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_language_files() -> LanguageFiles {
        LanguageFiles {
            common: vec![
                ".env".to_string(),
                ".env.*".to_string(),
                ".gitignore".to_string(),
            ],
            python: vec![
                "*_step.py".to_string(),
                "requirements.txt".to_string(),
                "pyproject.toml".to_string(),
            ],
            typescript: vec![
                "*.step.ts".to_string(),
                "*.step.tsx".to_string(),
                "*.config.ts".to_string(),
                "tsconfig.json".to_string(),
            ],
            javascript: vec!["*.step.js".to_string(), "*.step.jsx".to_string()],
            node: vec!["package.json".to_string()],
            rust: vec![],
        }
    }

    #[test]
    fn test_should_include_typescript_files() {
        let languages = vec![Language::TypeScript];
        let lf = test_language_files();

        assert!(should_include_file("src/start.step.ts", &languages, &lf));
        assert!(should_include_file("src/start.step.tsx", &languages, &lf));
        assert!(should_include_file(
            "src/tutorial.config.ts",
            &languages,
            &lf
        ));
        assert!(!should_include_file(
            "src/javascript.step.js",
            &languages,
            &lf
        ));
        assert!(!should_include_file("src/python_step.py", &languages, &lf));
    }

    #[test]
    fn test_common_files_always_included() {
        let languages = vec![Language::TypeScript];
        let lf = test_language_files();

        // Common files are always included
        assert!(should_include_file(".env", &languages, &lf));
        assert!(should_include_file(".env.local", &languages, &lf));
        assert!(should_include_file(".gitignore", &languages, &lf));
    }

    #[test]
    fn test_unlisted_files_excluded() {
        let languages = vec![Language::TypeScript];
        let lf = test_language_files();

        // Files not in any list should be excluded
        assert!(!should_include_file("random.txt", &languages, &lf));
        assert!(!should_include_file("unknown.file", &languages, &lf));
    }

    #[test]
    fn test_should_include_node_files() {
        let languages = vec![Language::TypeScript];
        let lf = test_language_files();

        // package.json requires node (JS or TS)
        assert!(should_include_file("package.json", &languages, &lf));
        // motia.config.ts is TypeScript-filtered
        assert!(should_include_file("motia.config.ts", &languages, &lf));
    }

    #[test]
    fn test_should_include_multiple_languages() {
        let languages = vec![Language::TypeScript, Language::Python];
        let lf = test_language_files();

        assert!(should_include_file("src/start.step.ts", &languages, &lf));
        assert!(should_include_file("src/python_step.py", &languages, &lf));
        assert!(!should_include_file(
            "src/javascript.step.js",
            &languages,
            &lf
        ));
    }

    #[test]
    fn test_python_only_files() {
        let ts_only = vec![Language::TypeScript];
        let py_only = vec![Language::Python];
        let lf = test_language_files();

        // requirements.txt should only be included with Python
        assert!(!should_include_file("requirements.txt", &ts_only, &lf));
        assert!(should_include_file("requirements.txt", &py_only, &lf));
    }
}
