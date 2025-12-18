use std::collections::HashMap;

use axum::http::HeaderMap;

pub fn headers_to_map(headers: &HeaderMap) -> HashMap<String, String> {
    headers
        .iter()
        .filter_map(|(k, v)| {
            v.to_str()
                .ok()
                .map(|v| (k.as_str().to_string(), v.to_string()))
        })
        .collect()
}

/// Converts `Uri::query()` into `HashMap<String, Vec<String>>`
///
/// Example:
/// ?a=1&a=2&b=3&c=
/// =>
/// {
///   "a": ["1", "2"],
///   "b": ["3"],
///   "c": [""]
/// }
pub fn query_to_multi_map(query: Option<&str>) -> HashMap<String, Vec<String>> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();

    let query = match query {
        Some(q) if !q.is_empty() => q,
        _ => return map,
    };

    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }

        let mut parts = pair.splitn(2, '=');
        let key = match parts.next() {
            Some(k) if !k.is_empty() => k,
            _ => continue,
        };

        let value = parts.next().unwrap_or("");

        map.entry(key.to_string())
            .or_default()
            .push(value.to_string());
    }

    map
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderName, HeaderValue};

    use super::*;

    #[test]
    fn test_headers_to_map() {
        let headers = HeaderMap::from_iter(vec![
            (
                HeaderName::from_static("authorization"),
                HeaderValue::from_static("Bearer 1234567890"),
            ),
            (
                HeaderName::from_static("content-type"),
                HeaderValue::from_static("application/json"),
            ),
            (
                HeaderName::from_static("user-agent"),
                HeaderValue::from_static("Mozilla/5.0"),
            ),
        ]);

        let map = headers_to_map(&headers);
        assert_eq!(
            map,
            HashMap::from([
                ("authorization".to_string(), "Bearer 1234567890".to_string()),
                ("content-type".to_string(), "application/json".to_string()),
                ("user-agent".to_string(), "Mozilla/5.0".to_string()),
            ])
        );
    }

    #[test]
    fn test_query_to_multi_map() {
        let query = "a=1&a=2&b=3&c=";
        let map = query_to_multi_map(Some(query));
        assert_eq!(
            map,
            HashMap::from([
                ("a".to_string(), vec!["1".to_string(), "2".to_string()]),
                ("b".to_string(), vec!["3".to_string()]),
                ("c".to_string(), vec!["".to_string()])
            ])
        );
    }
}
