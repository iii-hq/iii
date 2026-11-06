//! Heap the engine keeps after validating configuration values against their
//! schemas. jsonschema 0.30 cached every level of the draft meta-schema it
//! walked in static validators: two small nested schemas kept ~33 MiB for the
//! life of the process, ~70 MiB for the eight workers of a Harness stack.
//!
//! Own test binary: the statics must be cold, and the counting allocator is
//! global.

use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::{
        Arc,
        atomic::{AtomicIsize, Ordering},
    },
};

use iii::engine::Engine;
use iii::workers::configuration::ConfigurationWorker;
use iii::workers::configuration::adapters::ConfigurationAdapter;
use iii::workers::configuration::adapters::fs::FsAdapter;
use iii::workers::configuration::structs::ConfigurationRegisterInput;
use serde_json::{Value, json};

struct Counting;

static LIVE: AtomicIsize = AtomicIsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        LIVE.fetch_add(l.size() as isize, Ordering::Relaxed);
        unsafe { System.alloc(l) }
    }

    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        LIVE.fetch_sub(l.size() as isize, Ordering::Relaxed);
        unsafe { System.dealloc(p, l) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

/// Ceiling on what the registrations below may keep. The entries themselves
/// are a few KiB; jsonschema 0.30 kept ~33 MiB here.
const MAX_RETAINED: isize = 8 << 20;

/// A schemars-style draft-07 schema with `$ref`s into `definitions`.
fn draft07_schema() -> (Value, Value) {
    let schema = json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "pool": { "$ref": "#/definitions/Pool" },
            "targets": { "type": "array", "items": { "$ref": "#/definitions/Target" } }
        },
        "definitions": {
            "Pool": {
                "type": "object",
                "properties": {
                    "max": { "type": "integer", "minimum": 1 },
                    "idle_ms": { "type": ["integer", "null"], "format": "uint64" }
                }
            },
            "Target": {
                "type": "object",
                "required": ["name"],
                "properties": {
                    "name": { "type": "string", "minLength": 1 },
                    "mode": { "oneOf": [
                        { "type": "string", "enum": ["fast"] },
                        { "type": "string", "enum": ["safe"] }
                    ] },
                    "retry": { "anyOf": [{ "$ref": "#/definitions/Retry" }, { "type": "null" }] }
                }
            },
            "Retry": {
                "type": "object",
                "properties": {
                    "attempts": { "type": "integer" },
                    "backoff": {
                        "type": "object",
                        "properties": {
                            "base_ms": { "type": "integer" },
                            "max_ms": { "type": "integer" }
                        }
                    }
                }
            }
        }
    });
    let value = json!({
        "pool": { "max": 4 },
        "targets": [{ "name": "a", "mode": "fast", "retry": { "attempts": 2 } }]
    });
    (schema, value)
}

/// A nested schema with no `$schema`: the default draft.
fn default_draft_schema() -> (Value, Value) {
    let schema = json!({
        "type": "object",
        "properties": {
            "server": {
                "type": "object",
                "properties": {
                    "port": { "type": "integer" },
                    "tls": {
                        "type": "object",
                        "properties": {
                            "enabled": { "type": "boolean" },
                            "cert": { "type": ["string", "null"] },
                            "ciphers": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "name": { "type": "string" },
                                        "weight": { "type": "number" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    });
    let value = json!({ "server": { "port": 3111, "tls": { "enabled": false } } });
    (schema, value)
}

#[tokio::test(flavor = "current_thread")]
async fn validating_configuration_schemas_keeps_little_heap() {
    iii::workers::observability::metrics::ensure_default_meter();
    let dir = tempfile::tempdir().expect("tempdir");
    let adapter = Arc::new(
        FsAdapter::new(Some(json!({ "directory": dir.path().to_str().unwrap() })))
            .await
            .expect("fs adapter"),
    ) as Arc<dyn ConfigurationAdapter>;
    let worker = ConfigurationWorker::for_test(Arc::new(Engine::new()), adapter);

    let before = LIVE.load(Ordering::Relaxed);
    for (id, (schema, value)) in [
        ("draft07", draft07_schema()),
        ("default-draft", default_draft_schema()),
    ] {
        let registered = worker
            .register_fn(ConfigurationRegisterInput {
                id: id.into(),
                name: id.into(),
                description: "memory".into(),
                schema,
                initial_value: Some(value),
                metadata: None,
            })
            .await;
        assert!(
            matches!(registered, iii::function::FunctionResult::Success(_)),
            "register {id} failed"
        );
    }
    let retained = LIVE.load(Ordering::Relaxed) - before;

    eprintln!("retained after validating two schemas: {retained} bytes");
    assert!(
        retained < MAX_RETAINED,
        "validating two configuration schemas kept {retained} bytes (limit {MAX_RETAINED})"
    );
}
