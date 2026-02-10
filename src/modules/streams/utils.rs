// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

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

    #[test]
    fn test_query_to_multi_map_none() {
        let map = query_to_multi_map(None);
        assert_eq!(map.len(), 0);
    }

    #[test]
    fn test_query_to_multi_map_empty_string() {
        let map = query_to_multi_map(Some(""));
        assert_eq!(map.len(), 0);
    }

    #[test]
    fn test_query_to_multi_map_key_without_value() {
        let map = query_to_multi_map(Some("key"));
        assert_eq!(map.len(), 1);
        assert_eq!(map.get("key"), Some(&vec!["".to_string()]));
    }

    #[test]
    fn test_query_to_multi_map_multiple_keys_without_value() {
        let map = query_to_multi_map(Some("a&b&c"));
        assert_eq!(map.len(), 3);
        assert_eq!(map.get("a"), Some(&vec!["".to_string()]));
        assert_eq!(map.get("b"), Some(&vec!["".to_string()]));
        assert_eq!(map.get("c"), Some(&vec!["".to_string()]));
    }

    #[test]
    fn test_query_to_multi_map_trailing_ampersand() {
        let map = query_to_multi_map(Some("a=1&b=2&"));
        assert_eq!(map.len(), 2);
        assert_eq!(map.get("a"), Some(&vec!["1".to_string()]));
        assert_eq!(map.get("b"), Some(&vec!["2".to_string()]));
    }

    #[test]
    fn test_query_to_multi_map_leading_ampersand() {
        let map = query_to_multi_map(Some("&a=1"));
        assert_eq!(map.len(), 1);
        assert_eq!(map.get("a"), Some(&vec!["1".to_string()]));
    }

    #[test]
    fn test_query_to_multi_map_consecutive_ampersands() {
        let map = query_to_multi_map(Some("a=1&&b=2"));
        assert_eq!(map.len(), 2);
        assert_eq!(map.get("a"), Some(&vec!["1".to_string()]));
        assert_eq!(map.get("b"), Some(&vec!["2".to_string()]));
    }

    #[test]
    fn test_query_to_multi_map_duplicate_keys() {
        let map = query_to_multi_map(Some("a=1&a=2&a=3"));
        assert_eq!(map.len(), 1);
        assert_eq!(
            map.get("a"),
            Some(&vec!["1".to_string(), "2".to_string(), "3".to_string()])
        );
    }

    #[test]
    fn test_query_to_multi_map_url_encoded() {
        let map = query_to_multi_map(Some("key=hello%20world&name=test%2Fvalue"));
        assert_eq!(map.len(), 2);
        assert_eq!(map.get("key"), Some(&vec!["hello%20world".to_string()]));
        assert_eq!(map.get("name"), Some(&vec!["test%2Fvalue".to_string()]));
    }

    #[test]
    fn test_query_to_multi_map_empty_key_skipped() {
        let map = query_to_multi_map(Some("=value&key=test"));
        assert_eq!(map.len(), 1);
        assert_eq!(map.get("key"), Some(&vec!["test".to_string()]));
    }

    #[test]
    fn test_headers_to_map_non_utf8() {
        let mut headers = HeaderMap::new();
        let non_utf8 = HeaderValue::from_bytes(b"\xFF\xFE").unwrap();
        headers.insert("custom-header", non_utf8);
        headers.insert("valid-header", HeaderValue::from_static("valid-value"));

        let map = headers_to_map(&headers);
        assert_eq!(map.len(), 1);
        assert_eq!(map.get("valid-header"), Some(&"valid-value".to_string()));
        assert_eq!(map.get("custom-header"), None);
    }

    #[test]
    fn test_headers_to_map_empty() {
        let headers = HeaderMap::new();
        let map = headers_to_map(&headers);
        assert_eq!(map.len(), 0);
    }
}
