// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MiddlewarePhase {
    OnRequest,
    PreHandler,
    PostHandler,
    OnResponse,
    OnError,
    OnTimeout,
}

impl MiddlewarePhase {
    pub fn as_str(&self) -> &'static str {
        match self {
            MiddlewarePhase::OnRequest => "onRequest",
            MiddlewarePhase::PreHandler => "preHandler",
            MiddlewarePhase::PostHandler => "postHandler",
            MiddlewarePhase::OnResponse => "onResponse",
            MiddlewarePhase::OnError => "onError",
            MiddlewarePhase::OnTimeout => "onTimeout",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "onRequest" => Some(MiddlewarePhase::OnRequest),
            "preHandler" => Some(MiddlewarePhase::PreHandler),
            "postHandler" => Some(MiddlewarePhase::PostHandler),
            "onResponse" => Some(MiddlewarePhase::OnResponse),
            "onError" => Some(MiddlewarePhase::OnError),
            "onTimeout" => Some(MiddlewarePhase::OnTimeout),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MiddlewareScope {
    pub path: String,
}

impl MiddlewareScope {
    pub fn matches(&self, request_path: &str) -> bool {
        let pattern = self.path.trim_start_matches('/');
        let path = request_path.trim_start_matches('/');

        if pattern.is_empty() && path.is_empty() {
            return true;
        }
        if pattern.is_empty() || path.is_empty() {
            return false;
        }

        let pattern_segments: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
        let path_segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

        if pattern_segments.is_empty() {
            return path_segments.is_empty();
        }

        let last = pattern_segments.last().copied().unwrap_or("");
        if last == "*" {
            let prefix_len = pattern_segments.len() - 1;
            if path_segments.len() < prefix_len {
                return false;
            }
            for i in 0..prefix_len {
                let p = pattern_segments[i];
                let a = path_segments[i];
                if p != "*" && !p.starts_with(':') && p != a {
                    return false;
                }
            }
            true
        } else {
            if pattern_segments.len() != path_segments.len() {
                return false;
            }
            pattern_segments
                .iter()
                .zip(path_segments.iter())
                .all(|(p, a)| p.starts_with(':') || p == a)
        }
    }
}

#[derive(Debug, Clone)]
pub struct MiddlewareEntry {
    pub middleware_id: String,
    pub phase: MiddlewarePhase,
    pub scope: Option<MiddlewareScope>,
    pub priority: u16,
    pub function_id: String,
    pub worker_id: Uuid,
}

#[derive(Default)]
pub struct MiddlewareRegistry {
    entries: DashMap<MiddlewarePhase, Vec<MiddlewareEntry>>,
}

impl MiddlewareRegistry {
    pub fn new() -> Self {
        Self {
            entries: DashMap::new(),
        }
    }

    pub fn register(&self, entry: MiddlewareEntry) {
        self.entries
            .entry(entry.phase)
            .or_default()
            .push(entry.clone());
        if let Some(mut vec) = self.entries.get_mut(&entry.phase) {
            vec.sort_by_key(|e| e.priority);
        }
    }

    pub fn deregister(&self, middleware_id: &str) -> bool {
        let mut removed = false;
        for mut vec in self.entries.iter_mut() {
            let before = vec.len();
            vec.retain(|e| e.middleware_id != middleware_id);
            if vec.len() < before {
                removed = true;
            }
        }
        removed
    }

    pub fn deregister_by_worker(&self, worker_id: &Uuid) {
        for mut vec in self.entries.iter_mut() {
            vec.retain(|e| e.worker_id != *worker_id);
        }
    }

    pub fn get_matching(&self, phase: MiddlewarePhase, request_path: &str) -> Vec<MiddlewareEntry> {
        self.entries
            .get(&phase)
            .map(|vec| {
                vec.iter()
                    .filter(|e| {
                        e.scope
                            .as_ref()
                            .map(|s| s.matches(request_path))
                            .unwrap_or(true)
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::{MiddlewareEntry, MiddlewarePhase, MiddlewareRegistry, MiddlewareScope};
    use uuid::Uuid;

    #[test]
    fn scope_matches_exact_path() {
        let scope = MiddlewareScope {
            path: "/api/users".to_string(),
        };
        assert!(scope.matches("/api/users"));
        assert!(scope.matches("api/users"));
        assert!(!scope.matches("/api/users/123"));
        assert!(!scope.matches("/api"));
    }

    #[test]
    fn scope_matches_wildcard() {
        let scope = MiddlewareScope {
            path: "/api/*".to_string(),
        };
        assert!(scope.matches("/api/users"));
        assert!(scope.matches("/api/users/123"));
        assert!(scope.matches("/api"));
        assert!(!scope.matches("/other"));
    }

    #[test]
    fn scope_matches_param() {
        let scope = MiddlewareScope {
            path: "/api/users/:id".to_string(),
        };
        assert!(scope.matches("/api/users/123"));
        assert!(scope.matches("/api/users/abc"));
        assert!(!scope.matches("/api/users"));
        assert!(!scope.matches("/api/users/123/extra"));
    }

    #[test]
    fn registry_register_and_get_matching() {
        let registry = MiddlewareRegistry::new();
        let worker_id = Uuid::new_v4();
        registry.register(MiddlewareEntry {
            middleware_id: "m1".to_string(),
            phase: MiddlewarePhase::PreHandler,
            scope: Some(MiddlewareScope {
                path: "/api/*".to_string(),
            }),
            priority: 10,
            function_id: "f1".to_string(),
            worker_id,
        });
        let entries = registry.get_matching(MiddlewarePhase::PreHandler, "/api/users");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].middleware_id, "m1");
    }

    #[test]
    fn registry_deregister() {
        let registry = MiddlewareRegistry::new();
        let worker_id = Uuid::new_v4();
        registry.register(MiddlewareEntry {
            middleware_id: "m1".to_string(),
            phase: MiddlewarePhase::PreHandler,
            scope: None,
            priority: 10,
            function_id: "f1".to_string(),
            worker_id,
        });
        assert!(registry.deregister("m1"));
        assert!(
            registry
                .get_matching(MiddlewarePhase::PreHandler, "/any")
                .is_empty()
        );
    }

    #[test]
    fn registry_priority_ordering() {
        let registry = MiddlewareRegistry::new();
        let worker_id = Uuid::new_v4();
        registry.register(MiddlewareEntry {
            middleware_id: "m2".to_string(),
            phase: MiddlewarePhase::OnRequest,
            scope: None,
            priority: 200,
            function_id: "f2".to_string(),
            worker_id,
        });
        registry.register(MiddlewareEntry {
            middleware_id: "m1".to_string(),
            phase: MiddlewarePhase::OnRequest,
            scope: None,
            priority: 50,
            function_id: "f1".to_string(),
            worker_id,
        });
        let entries = registry.get_matching(MiddlewarePhase::OnRequest, "/any");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].middleware_id, "m1");
        assert_eq!(entries[1].middleware_id, "m2");
    }

    #[test]
    fn registry_deregister_by_worker() {
        let registry = MiddlewareRegistry::new();
        let w1 = Uuid::new_v4();
        let w2 = Uuid::new_v4();
        registry.register(MiddlewareEntry {
            middleware_id: "m1".to_string(),
            phase: MiddlewarePhase::PreHandler,
            scope: None,
            priority: 10,
            function_id: "f1".to_string(),
            worker_id: w1,
        });
        registry.register(MiddlewareEntry {
            middleware_id: "m2".to_string(),
            phase: MiddlewarePhase::PreHandler,
            scope: None,
            priority: 10,
            function_id: "f2".to_string(),
            worker_id: w2,
        });
        registry.deregister_by_worker(&w1);
        let entries = registry.get_matching(MiddlewarePhase::PreHandler, "/any");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].middleware_id, "m2");
    }
}
