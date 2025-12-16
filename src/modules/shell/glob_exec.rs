use std::path::Path;

#[derive(Debug, Clone)]
pub struct GlobExec {
    patterns: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchRoot {
    pub path: String,
    pub recursive: bool,
}

impl GlobExec {
    pub fn new(patterns: Vec<String>) -> Self {
        Self { patterns }
    }

    /// Extract directories to watch and whether they are recursive
    pub fn watch_roots(&self) -> Vec<WatchRoot> {
        let mut roots = Vec::new();

        for pattern in &self.patterns {
            let (root, recursive) = Self::extract_root(pattern);
            roots.push(WatchRoot {
                path: root,
                recursive,
            });
        }

        roots.sort_by(|a, b| a.path.cmp(&b.path));
        roots.dedup_by(|a, b| a.path == b.path);

        roots
    }

    /// Check whether a filesystem path should trigger a restart
    pub fn should_trigger(&self, path: &Path) -> bool {
        self.patterns
            .iter()
            .any(|pattern| Self::matches_pattern(pattern, path))
    }

    // ──────────────────────────────────────────────────────────────
    // Internal helpers
    // ──────────────────────────────────────────────────────────────

    fn extract_root(pattern: &str) -> (String, bool) {
        let recursive = pattern.contains("**");

        let root = pattern
            .split("**")
            .next()
            .unwrap_or(pattern)
            .split('*')
            .next()
            .unwrap_or(pattern)
            .trim_end_matches('/')
            .trim_end_matches('\\');

        let root = if root.is_empty() { "." } else { root };

        (root.to_string(), recursive)
    }

    fn matches_pattern(pattern: &str, path: &Path) -> bool {
        let (root, _recursive) = Self::extract_root(pattern);

        // Normalize root
        let root_path = Path::new(&root);

        // Path must be under the root directory
        if !path.starts_with(root_path) {
            return false;
        }

        let ext = path.extension().and_then(|e| e.to_str());

        let Some(ext) = ext else {
            return false;
        };

        // Extension filtering
        if pattern.contains('{') {
            if let Some(list) = Self::extract_extension_group(pattern) {
                return list.iter().any(|e| *e == ext);
            }
        } else if let Some(expected) = Self::extract_single_extension(pattern) {
            return expected == ext;
        }

        false
    }

    fn extract_single_extension(pattern: &str) -> Option<&str> {
        pattern
            .rsplit('.')
            .next()
            .filter(|s| !s.contains('*') && !s.contains('{'))
    }

    fn extract_extension_group(pattern: &str) -> Option<Vec<&str>> {
        let start = pattern.find('{')?;
        let end = pattern.find('}')?;

        Some(
            pattern[start + 1..end]
                .split(',')
                .map(|s| s.trim())
                .collect(),
        )
    }
}

#[test]
fn extract_recursive_root() {
    let g = GlobExec::new(vec!["steps/**/*.{ts,js}".into()]);
    let roots = g.watch_roots();

    assert_eq!(
        roots,
        vec![WatchRoot {
            path: "steps".into(),
            recursive: true
        }]
    );
}

#[test]
fn extract_non_recursive_root() {
    let g = GlobExec::new(vec!["config/*.json".into()]);
    let roots = g.watch_roots();

    assert_eq!(
        roots,
        vec![WatchRoot {
            path: "config".into(),
            recursive: false
        }]
    );
}

#[test]
fn match_single_extension() {
    let g = GlobExec::new(vec!["src/**/*.ts".into()]);

    assert!(g.should_trigger(Path::new("src/main.ts")));
    assert!(!g.should_trigger(Path::new("src/main.js")));
}

#[test]
fn match_extension_group() {
    let g = GlobExec::new(vec!["steps/**/*.{ts,js}".into()]);

    assert!(g.should_trigger(Path::new("steps/a.ts")));
    assert!(g.should_trigger(Path::new("steps/a.js")));
    assert!(!g.should_trigger(Path::new("steps/a.rs")));
}

#[test]
fn multiple_patterns() {
    let g = GlobExec::new(vec!["steps/**/*.ts".into(), "src/**/*.js".into()]);

    assert!(g.should_trigger(Path::new("steps/a.ts")));
    assert!(g.should_trigger(Path::new("src/a.js")));
    assert!(!g.should_trigger(Path::new("src/a.ts")));
}
