//! Template manifest types and parsing

use serde::{Deserialize, Serialize};

/// File patterns associated with each language
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LanguageFiles {
    /// Files always included regardless of language selection
    #[serde(default)]
    pub common: Vec<String>,

    /// Files that require Python to be selected
    #[serde(default)]
    pub python: Vec<String>,

    /// Files that require TypeScript to be selected
    #[serde(default)]
    pub typescript: Vec<String>,

    /// Files that require JavaScript to be selected
    #[serde(default)]
    pub javascript: Vec<String>,

    /// Files that require either JavaScript or TypeScript
    #[serde(default)]
    pub node: Vec<String>,

    /// Files that require Rust to be selected
    #[serde(default)]
    pub rust: Vec<String>,
}

impl LanguageFiles {
    /// Merge another LanguageFiles into this one (other takes precedence for additions)
    pub fn merge(&mut self, other: &LanguageFiles) {
        self.common.extend(other.common.iter().cloned());
        self.python.extend(other.python.iter().cloned());
        self.typescript.extend(other.typescript.iter().cloned());
        self.javascript.extend(other.javascript.iter().cloned());
        self.node.extend(other.node.iter().cloned());
        self.rust.extend(other.rust.iter().cloned());
    }

    /// Check if a filename matches any pattern in a list
    fn matches_any(filename: &str, patterns: &[String]) -> bool {
        patterns.iter().any(|pattern| {
            if pattern.starts_with('*') {
                // Suffix match: *.ts matches foo.ts
                filename.ends_with(&pattern[1..])
            } else if pattern.ends_with('*') {
                // Prefix match: requirements* matches requirements.txt
                filename.starts_with(&pattern[..pattern.len() - 1])
            } else {
                // Exact match
                filename == pattern
            }
        })
    }

    /// Determine which language(s) a file is associated with
    /// Returns None if the file is not in any list (should be excluded)
    /// Returns Some(FileLanguage::Common) if in the common list
    pub fn get_language_for_file(&self, file_path: &str) -> Option<FileLanguage> {
        let filename = file_path.rsplit('/').next().unwrap_or(file_path);

        // Check common first - always included
        if Self::matches_any(filename, &self.common) {
            return Some(FileLanguage::Common);
        }
        if Self::matches_any(filename, &self.python) {
            return Some(FileLanguage::Python);
        }
        if Self::matches_any(filename, &self.typescript) {
            return Some(FileLanguage::TypeScript);
        }
        if Self::matches_any(filename, &self.javascript) {
            return Some(FileLanguage::JavaScript);
        }
        if Self::matches_any(filename, &self.node) {
            return Some(FileLanguage::Node);
        }
        if Self::matches_any(filename, &self.rust) {
            return Some(FileLanguage::Rust);
        }

        None // Not in any list, exclude by default
    }
}

/// Which language a file is associated with
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileLanguage {
    Common, // Always included
    Python,
    TypeScript,
    JavaScript,
    Node, // Either JS or TS
    Rust,
}

/// A shared file from the root templates directory that gets bundled into every template
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedFile {
    /// Source path relative to templates/ directory
    pub source: String,

    /// Destination path in each template (defaults to source if not specified)
    #[serde(default)]
    pub dest: Option<String>,
}

impl SharedFile {
    /// Get the destination path (falls back to source if dest not specified)
    pub fn destination(&self) -> &str {
        self.dest.as_deref().unwrap_or(&self.source)
    }
}

/// Root template manifest (templates/template.yaml)
/// Lists available template directories and global language file associations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RootManifest {
    /// List of template directory names
    pub templates: Vec<String>,

    /// Global language-specific file patterns (optional)
    #[serde(default)]
    pub language_files: LanguageFiles,

    /// Shared files from root templates/ directory to include in every template
    /// Supports renaming via source/dest mapping
    #[serde(default)]
    pub shared_files: Vec<SharedFile>,
}

/// Per-template manifest (templates/<name>/template.yaml)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateManifest {
    /// Display name of the template
    pub name: String,

    /// Description of what the template provides
    pub description: String,

    /// Semver version for CLI compatibility checking
    pub version: String,

    /// Minimum iii engine version required (checked via `iii --version`)
    /// When set, the CLI will hard-block if the installed iii version is too old.
    #[serde(default)]
    pub min_iii_version: Option<String>,

    /// Languages that must be included (user cannot deselect)
    #[serde(default)]
    pub requires: Vec<String>,

    /// Languages that can optionally be included
    #[serde(default)]
    pub optional: Vec<String>,

    /// When true, "required" languages become "included": always selected (not deselectable),
    /// and runtime checks become advisory (indicate availability, no hard fail)
    #[serde(default, alias = "treat_required_as_suggested")]
    pub treat_required_as_included: bool,

    /// Explicit list of files to copy
    pub files: Vec<String>,

    /// Template-specific language file overrides (merged with root)
    #[serde(default)]
    pub language_files: LanguageFiles,

    /// Post-creation steps shown to the user (after the auto-generated `cd` step)
    #[serde(default)]
    pub next_steps: Vec<String>,
}

impl TemplateManifest {
    /// Check if a language is required by this template
    pub fn is_required(&self, language: &str) -> bool {
        self.requires
            .iter()
            .any(|r| r.eq_ignore_ascii_case(language))
    }

    /// Check if a language is optional for this template
    pub fn is_optional(&self, language: &str) -> bool {
        self.optional
            .iter()
            .any(|o| o.eq_ignore_ascii_case(language))
    }

    /// When treat_required_as_included is true, required languages become included
    /// (always selected, advisory runtime check). Returns names of those languages.
    pub fn included_language_names(&self) -> Vec<&str> {
        if self.treat_required_as_included {
            self.requires.iter().map(String::as_str).collect()
        } else {
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_manifest_yaml(next_steps: Option<&str>) -> String {
        let mut yaml = String::from(
            "name: Test\ndescription: A test template\nversion: '0.1.0'\nfiles:\n  - README.md\n",
        );
        if let Some(steps) = next_steps {
            yaml.push_str(steps);
        }
        yaml
    }

    #[test]
    fn next_steps_defaults_to_empty_when_absent() {
        let manifest: TemplateManifest =
            serde_yaml::from_str(&minimal_manifest_yaml(None)).unwrap();
        assert!(manifest.next_steps.is_empty());
    }

    #[test]
    fn next_steps_parses_from_yaml() {
        let manifest: TemplateManifest = serde_yaml::from_str(&minimal_manifest_yaml(Some(
            "next_steps:\n  - Continue with the quickstart at https://iii.dev/docs/quickstart\n",
        )))
        .unwrap();
        assert_eq!(manifest.next_steps.len(), 1);
        assert_eq!(
            manifest.next_steps[0],
            "Continue with the quickstart at https://iii.dev/docs/quickstart"
        );
    }

    #[test]
    fn next_steps_parses_multiple_entries() {
        let manifest: TemplateManifest = serde_yaml::from_str(&minimal_manifest_yaml(Some(
            "next_steps:\n  - Step one\n  - Step two\n  - Step three\n",
        )))
        .unwrap();
        assert_eq!(
            manifest.next_steps,
            vec!["Step one", "Step two", "Step three"]
        );
    }
}
