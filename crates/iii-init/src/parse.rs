//! Pure parsers used by iii-init, compiled on all platforms so they can be
//! unit-tested from the build host (iii-init itself is a Linux-guest binary).

/// Parse the `III_VIRTIOFS_MOUNTS` env-var spec into `(tag, guest_path)` pairs.
///
/// Format: `tag1=/guest/path1;tag2=/guest/path2`. Empty segments and segments
/// without a valid `tag=path` split are dropped (the caller prints a warning
/// via `report` when a malformed entry is encountered). An empty spec yields
/// an empty vec.
///
/// `report` receives each rejected entry verbatim so callers can log without
/// this function needing access to stderr.
pub fn parse_virtiofs_spec<F: FnMut(&str)>(spec: &str, mut report: F) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for entry in spec.split(';') {
        if entry.is_empty() {
            continue;
        }
        match entry.split_once('=') {
            Some((tag, guest_path)) if !tag.is_empty() && !guest_path.is_empty() => {
                out.push((tag.to_string(), guest_path.to_string()));
            }
            _ => report(entry),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(spec: &str) -> (Vec<(String, String)>, Vec<String>) {
        let mut rejected = Vec::new();
        let parsed = parse_virtiofs_spec(spec, |e| rejected.push(e.to_string()));
        (parsed, rejected)
    }

    #[test]
    fn empty_spec_yields_empty_vec() {
        let (parsed, rejected) = parse("");
        assert!(parsed.is_empty());
        assert!(rejected.is_empty());
    }

    #[test]
    fn single_entry_parses() {
        let (parsed, rejected) = parse("virtiofs_0=/workspace");
        assert_eq!(
            parsed,
            vec![("virtiofs_0".to_string(), "/workspace".to_string())]
        );
        assert!(rejected.is_empty());
    }

    #[test]
    fn multiple_entries_preserve_order() {
        let (parsed, _) = parse("virtiofs_0=/workspace;virtiofs_1=/data;virtiofs_2=/cache");
        assert_eq!(parsed.len(), 3);
        assert_eq!(
            parsed[0],
            ("virtiofs_0".to_string(), "/workspace".to_string())
        );
        assert_eq!(parsed[1], ("virtiofs_1".to_string(), "/data".to_string()));
        assert_eq!(parsed[2], ("virtiofs_2".to_string(), "/cache".to_string()));
    }

    #[test]
    fn trailing_semicolon_is_tolerated() {
        let (parsed, rejected) = parse("virtiofs_0=/workspace;");
        assert_eq!(parsed.len(), 1);
        assert!(rejected.is_empty());
    }

    #[test]
    fn leading_semicolon_is_tolerated() {
        let (parsed, rejected) = parse(";virtiofs_0=/workspace");
        assert_eq!(parsed.len(), 1);
        assert!(rejected.is_empty());
    }

    #[test]
    fn consecutive_semicolons_are_tolerated() {
        let (parsed, _) = parse("virtiofs_0=/a;;virtiofs_1=/b");
        assert_eq!(parsed.len(), 2);
    }

    #[test]
    fn missing_equals_is_rejected() {
        let (parsed, rejected) = parse("virtiofs_0/workspace");
        assert!(parsed.is_empty());
        assert_eq!(rejected, vec!["virtiofs_0/workspace".to_string()]);
    }

    #[test]
    fn empty_tag_is_rejected() {
        let (parsed, rejected) = parse("=/workspace");
        assert!(parsed.is_empty());
        assert_eq!(rejected, vec!["=/workspace".to_string()]);
    }

    #[test]
    fn empty_path_is_rejected() {
        let (parsed, rejected) = parse("virtiofs_0=");
        assert!(parsed.is_empty());
        assert_eq!(rejected, vec!["virtiofs_0=".to_string()]);
    }

    #[test]
    fn extra_equals_in_path_is_kept_as_path() {
        // Paths don't contain `=` on Linux, but if one does, split_once
        // keeps the first `=` as the separator — second `=` stays in the value.
        let (parsed, rejected) = parse("virtiofs_0=/path=with=equals");
        assert_eq!(
            parsed,
            vec![("virtiofs_0".to_string(), "/path=with=equals".to_string())]
        );
        assert!(rejected.is_empty());
    }

    #[test]
    fn malformed_and_valid_entries_coexist() {
        let (parsed, rejected) = parse("virtiofs_0=/workspace;garbage;virtiofs_1=/data;=/bad");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].0, "virtiofs_0");
        assert_eq!(parsed[1].0, "virtiofs_1");
        assert_eq!(rejected, vec!["garbage".to_string(), "=/bad".to_string()]);
    }
}
