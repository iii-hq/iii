use serde_json::Value;

use crate::Doc;
use crate::error::FilterError;
use crate::limits::MAX_PATH_SEGMENTS;

/// The first segment of a path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Root {
    Input,
    Output,
    Error,
    Caller,
    Metadata,
    /// `@`: the current array element, only inside `$any`.
    Element,
}

impl Root {
    fn from_segment(segment: &str) -> Option<Root> {
        Some(match segment {
            "input" => Root::Input,
            "output" => Root::Output,
            "error" => Root::Error,
            "caller" => Root::Caller,
            "metadata" => Root::Metadata,
            "@" => Root::Element,
            _ => return None,
        })
    }
}

/// One segment after the root. `index` is set when the segment is a canonical
/// decimal (`0` or `[1-9][0-9]*`) that fits a `usize`; it is used on arrays,
/// while `key` is used on objects.
#[derive(Debug)]
pub(crate) struct Segment {
    pub key: Box<str>,
    pub index: Option<usize>,
}

#[derive(Debug)]
pub(crate) struct Path {
    pub root: Root,
    pub segments: Box<[Segment]>,
}

impl Path {
    /// Parses `root ("." segment)*`. `\.` and `\\` escape inside keys; any
    /// other backslash sequence, an empty segment or an unknown root is
    /// `INVALID_FILTER`. `@` is a root only inside `$any`, where it is the
    /// only root allowed.
    pub fn parse(raw: &str, in_any: bool) -> Result<Path, FilterError> {
        let mut parts: Vec<String> = Vec::new();
        let mut current = String::new();
        let mut chars = raw.chars();
        while let Some(c) = chars.next() {
            match c {
                '\\' => match chars.next() {
                    Some(escaped @ ('.' | '\\')) => current.push(escaped),
                    Some(other) => {
                        return Err(FilterError::invalid(format!(
                            "path `{raw}`: invalid escape `\\{other}` (only `\\.` and `\\\\` are allowed)"
                        )));
                    }
                    None => {
                        return Err(FilterError::invalid(format!(
                            "path `{raw}`: dangling `\\` at the end"
                        )));
                    }
                },
                '.' => {
                    if current.is_empty() {
                        return Err(empty_segment(raw));
                    }
                    parts.push(std::mem::take(&mut current));
                }
                c => current.push(c),
            }
        }
        if current.is_empty() {
            return Err(empty_segment(raw));
        }
        parts.push(current);

        let mut parts = parts.into_iter();
        let first = parts.next().expect("at least one segment");
        let root = match Root::from_segment(&first) {
            Some(Root::Element) if !in_any => {
                return Err(FilterError::invalid(format!(
                    "path `{raw}`: `@` paths are only allowed inside `$any`"
                )));
            }
            Some(root) if in_any && root != Root::Element => {
                return Err(FilterError::invalid(format!(
                    "path `{raw}`: inside `$any` paths start at `@`, the current element"
                )));
            }
            Some(root) => root,
            None if in_any => {
                return Err(FilterError::invalid(format!(
                    "path `{raw}`: inside `$any` paths start at `@`, the current element"
                )));
            }
            None => {
                return Err(FilterError::invalid(format!(
                    "path `{raw}`: unknown root `{first}` (expected input, output, error, caller or metadata)"
                )));
            }
        };

        let segments: Box<[Segment]> = parts
            .map(|key| Segment {
                index: canonical_index(&key),
                key: key.into_boxed_str(),
            })
            .collect();
        if segments.len() > MAX_PATH_SEGMENTS {
            return Err(FilterError::invalid(format!(
                "path `{raw}`: {} segments after the root (limit {MAX_PATH_SEGMENTS})",
                segments.len()
            )));
        }

        Ok(Path { root, segments })
    }

    /// Resolves the path against the document; `None` is *absent*.
    #[inline]
    pub fn resolve<'a>(&self, doc: &Doc<'a>, element: Option<&'a Value>) -> Option<&'a Value> {
        let mut current = match self.root {
            Root::Input => doc.input,
            Root::Output => doc.output,
            Root::Error => doc.error,
            Root::Caller => doc.caller,
            Root::Metadata => doc.metadata,
            Root::Element => element,
        }?;
        for segment in self.segments.iter() {
            current = match current {
                Value::Object(map) => map.get(&*segment.key)?,
                Value::Array(items) => items.get(segment.index?)?,
                _ => return None,
            };
        }
        Some(current)
    }
}

fn empty_segment(raw: &str) -> FilterError {
    if raw.is_empty() {
        FilterError::invalid("empty path")
    } else {
        FilterError::invalid(format!("path `{raw}`: empty segment"))
    }
}

fn canonical_index(segment: &str) -> Option<usize> {
    let bytes = segment.as_bytes();
    let canonical = match bytes {
        [b'0'] => true,
        [b'1'..=b'9', rest @ ..] => rest.iter().all(u8::is_ascii_digit),
        _ => false,
    };
    if canonical {
        segment.parse().ok()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(path: &Path) -> Vec<&str> {
        path.segments.iter().map(|s| &*s.key).collect()
    }

    #[test]
    fn escapes() {
        let p = Path::parse(r"input.a\.b.c\\d", false).unwrap();
        assert_eq!(p.root, Root::Input);
        assert_eq!(keys(&p), vec!["a.b", r"c\d"]);
        assert!(Path::parse(r"input.a\b", false).is_err());
        assert!(Path::parse(r"input.a\", false).is_err());
    }

    #[test]
    fn indexes() {
        let p = Path::parse("input.0.01.10.x.99999999999999999999999", false).unwrap();
        let idx: Vec<Option<usize>> = p.segments.iter().map(|s| s.index).collect();
        assert_eq!(idx, vec![Some(0), None, Some(10), None, None]);
    }

    #[test]
    fn roots() {
        assert!(Path::parse("@.a", false).is_err());
        assert!(Path::parse("input.a", true).is_err());
        assert_eq!(Path::parse("@", true).unwrap().root, Root::Element);
        assert_eq!(Path::parse("input.@", false).unwrap().segments.len(), 1);
        assert!(Path::parse("inputs.a", false).is_err());
        assert!(Path::parse("", false).is_err());
        assert!(Path::parse("input.", false).is_err());
        assert!(Path::parse(".input", false).is_err());
        assert!(Path::parse("input..a", false).is_err());
    }
}
