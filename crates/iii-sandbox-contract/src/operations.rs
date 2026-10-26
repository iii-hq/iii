// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

//! The sixteen ephemeral operations (the router's public `sandbox::*` ids, each also an
//! adapter function), the adapter id helpers and the worker-VM function suffixes.

use crate::ephemeral::*;
use schemars::JsonSchema;
use serde_json::Value;

pub const PUBLIC_PREFIX: &str = "sandbox::";
pub const DEFAULT_ADAPTER: &str = "sandbox-libkrun";
pub const ADAPTER_INFO_SUFFIX: &str = "adapter::info";
pub const ROUTER_INFO_ID: &str = "sandbox::adapter::info";

/// Worker-VM suffixes: Compose calls `adapter_function_id(adapter, WORKER_START)` etc.
pub const WORKER_PREPARE: &str = "worker::prepare";
pub const WORKER_START: &str = "worker::start";
pub const WORKER_STATUS: &str = "worker::status";
pub const WORKER_LOGS: &str = "worker::logs";
pub const WORKER_STOP: &str = "worker::stop";
pub const WORKER_DESTROY: &str = "worker::destroy";

/// `("sandbox-libkrun", "sandbox::fs::ls" | "fs::ls")` -> `"sandbox-libkrun::fs::ls"`;
/// strips a leading `PUBLIC_PREFIX`.
pub fn adapter_function_id(adapter: &str, id: &str) -> String {
    format!(
        "{adapter}::{}",
        id.strip_prefix(PUBLIC_PREFIX).unwrap_or(id)
    )
}

/// `[a-z0-9-]+`, and not `sandbox`, which would forward the router to itself.
pub fn is_valid_adapter_name(name: &str) -> bool {
    !name.is_empty()
        && name != "sandbox"
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// One ephemeral operation: its id suffix, description and the schemas of its request and response.
#[derive(Debug, Clone, Copy)]
pub struct Operation {
    /// `"create"`, `"fs::ls"`.
    pub suffix: &'static str,
    pub description: &'static str,
    pub request_schema: fn() -> Value,
    pub response_schema: fn() -> Value,
}

impl Operation {
    /// `PUBLIC_PREFIX` + `suffix`.
    pub fn public_id(&self) -> String {
        format!("{PUBLIC_PREFIX}{}", self.suffix)
    }
}

// Same conversion as `engine/function-macros/src/lib.rs` uses for `request_format`.
fn schema<T: JsonSchema>() -> Value {
    serde_json::to_value(schemars::schema_for!(T)).unwrap()
}

macro_rules! op {
    ($suffix:literal, $req:ty, $res:ty, $desc:literal) => {
        Operation {
            suffix: $suffix,
            description: $desc,
            request_schema: schema::<$req>,
            response_schema: schema::<$res>,
        }
    };
}

/// The 16 ephemeral operations in the daemon's registration order.
pub const OPERATIONS: &[Operation] = &[
    op!(
        "create",
        CreateRequest,
        CreateResponse,
        "Create an ephemeral sandbox VM. `image` must be a preset \
         (`\"python\"`, `\"node\"`) or a key from \
         the adapter's `custom_images` config; \
         OCI refs are NOT accepted unless they match a catalog key. \
         `env` accepts both `Vec<\"K=V\">` and `{ K: V }` map shapes."
    ),
    op!(
        "exec",
        ExecRequest,
        ExecResponse,
        "Execute a command inside a live sandbox. `cmd` accepts three shapes: \
         (1) a shell-style line that gets shlex-split (`cmd: \"node -v\"`), \
         (2) `cmd` + `args` (the POSIX shape, `cmd: \"node\", args: [\"-v\"]`), \
         (3) an `argv` array (`argv: [\"node\", \"-v\"]`). \
         Shell metacharacters in the shell-line shape are NOT interpreted; \
         use `sandbox::run` with `lang: \"shell\"` for bash semantics. \
         `env` accepts both `Vec<\"K=V\">` and `{ K: V }` map shapes."
    ),
    op!(
        "stop",
        StopRequest,
        StopResponse,
        "Stop and remove a running sandbox. Set `wait: true` to block \
         until the VM process exits and resources are reclaimed."
    ),
    op!("list", ListRequest, ListResponse, "List active sandboxes"),
    op!(
        "catalog::list",
        CatalogListRequest,
        CatalogListResponse,
        "List bootable images: bundled presets plus operator-registered custom_images. \
         Call this before sandbox::create when you don't know what's available."
    ),
    op!(
        "run",
        RunRequest,
        RunResponse,
        "Run code in an ephemeral sandbox in ONE call. \
         Composes create + fs::write + exec + stop. \
         `lang` selects the interpreter (`node`, `python`, `shell`, or a custom binary path). \
         Sandbox auto-stops on success and on failure unless `keep_sandbox: true`. \
         Example: { image: \"node\", code: \"console.log('hi')\", lang: \"node\" }"
    ),
    op!(
        "fs::ls",
        LsRequest,
        LsResponse,
        "List directory contents inside a sandbox. \
         Example: { sandbox_id: \"...\", path: \"/home/app\" }"
    ),
    op!(
        "fs::stat",
        StatRequest,
        StatResponse,
        "Stat a path inside a sandbox. \
         Example: { sandbox_id: \"...\", path: \"/home/app/index.js\" }"
    ),
    op!(
        "fs::mkdir",
        MkdirRequest,
        MkdirResponse,
        "Create a directory inside a sandbox. Pass `parents:true` to make missing parents \
         like `mkdir -p`. Example: { sandbox_id: \"...\", path: \"/home/app/cache\", mode: \"0755\", parents: true }"
    ),
    op!(
        "fs::rm",
        RmRequest,
        RmResponse,
        "Remove a file or directory inside a sandbox. Pass `recursive:true` to remove \
         directories with contents. Example: { sandbox_id: \"...\", path: \"/home/app/temp\", recursive: true }"
    ),
    op!(
        "fs::chmod",
        ChmodRequest,
        ChmodResponse,
        "Change file permissions (and optionally owner) inside a sandbox. \
         Example: { sandbox_id: \"...\", path: \"/home/app/script.sh\", mode: \"0755\" }"
    ),
    op!(
        "fs::mv",
        MvRequest,
        MvResponse,
        "Move or rename a path inside a sandbox. Pass `overwrite:true` to clobber an \
         existing destination. Example: { sandbox_id: \"...\", src: \"/home/app/old.js\", dst: \"/home/app/new.js\" }"
    ),
    op!(
        "fs::grep",
        GrepRequest,
        GrepResponse,
        "Search for a regex pattern in files inside a sandbox. Walks `path` recursively \
         by default. Example: { sandbox_id: \"...\", path: \"/home/app/src\", pattern: \"TODO\" }"
    ),
    op!(
        "fs::sed",
        SedRequest,
        SedResponse,
        "Find-and-replace in files inside a sandbox. Pass either `path` (walked like grep) \
         OR `files` (explicit list), not both. Example: { sandbox_id: \"...\", path: \"/home/app/src\", pattern: \"foo\", replacement: \"bar\" }"
    ),
    op!(
        "fs::write",
        WriteRequest,
        WriteResponse,
        "Write a file into a sandbox. `content` accepts a UTF-8 string (recommended for \
         source/text), a StreamChannelRef object (for large uploads), or use `content_b64` \
         for small binary. Example: { sandbox_id: \"...\", path: \"/home/app/index.js\", content: \"console.log('hi')\\n\" }"
    ),
    op!(
        "fs::read",
        ReadRequest,
        ReadResponse,
        "Read a file from a sandbox. Always returns `content`: a StreamChannelRef \
         callers can subscribe to for the full file bytes. For UTF-8 text files \
         under 1 MiB, the response also carries an inline `body` string so callers \
         can short-circuit the subscription. Example: { sandbox_id: \"...\", path: \"/home/app/index.js\" }"
    ),
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn operations_follow_daemon_order() {
        let s: Vec<_> = OPERATIONS.iter().map(|o| o.suffix).collect();
        assert_eq!(
            s,
            [
                "create",
                "exec",
                "stop",
                "list",
                "catalog::list",
                "run",
                "fs::ls",
                "fs::stat",
                "fs::mkdir",
                "fs::rm",
                "fs::chmod",
                "fs::mv",
                "fs::grep",
                "fs::sed",
                "fs::write",
                "fs::read"
            ]
        );
        assert_eq!(
            [
                WORKER_PREPARE,
                WORKER_START,
                WORKER_STATUS,
                WORKER_LOGS,
                WORKER_STOP,
                WORKER_DESTROY
            ],
            [
                "worker::prepare",
                "worker::start",
                "worker::status",
                "worker::logs",
                "worker::stop",
                "worker::destroy"
            ]
        );
    }

    #[test]
    fn ids() {
        assert_eq!(OPERATIONS[0].public_id(), "sandbox::create");
        assert_eq!(
            ROUTER_INFO_ID,
            format!("{PUBLIC_PREFIX}{ADAPTER_INFO_SUFFIX}")
        );
        assert_eq!(
            adapter_function_id("sandbox-libkrun", "sandbox::fs::ls"),
            "sandbox-libkrun::fs::ls"
        );
        assert_eq!(
            adapter_function_id("sandbox-libkrun", "fs::ls"),
            "sandbox-libkrun::fs::ls"
        );
        assert_eq!(
            adapter_function_id("fake", ADAPTER_INFO_SUFFIX),
            "fake::adapter::info"
        );
        assert_eq!(DEFAULT_ADAPTER, "sandbox-libkrun");
    }

    #[test]
    fn adapter_names() {
        for ok in ["sandbox-libkrun", "fake", "sandbox-mock", "a1"] {
            assert!(is_valid_adapter_name(ok), "{ok}");
        }
        for bad in ["", "sandbox", "Sandbox", "a_b", "a::b", "a b"] {
            assert!(!is_valid_adapter_name(bad), "{bad}");
        }
    }

    #[test]
    fn schemas_and_descriptions() {
        for op in OPERATIONS {
            assert!(!op.description.is_empty(), "{}", op.suffix);
            assert!(
                !op.description.contains("engine.workers.iii-sandbox"),
                "{}",
                op.suffix
            );
            for schema in [(op.request_schema)(), (op.response_schema)()] {
                assert!(
                    schema.is_object() && schema.get("$schema").is_some(),
                    "{}",
                    op.suffix
                );
                assert!(
                    !schema.to_string().contains("engine.workers.iii-sandbox"),
                    "{}",
                    op.suffix
                );
            }
        }

        let exec = OPERATIONS.iter().find(|o| o.suffix == "exec").unwrap();
        let schema = (exec.request_schema)();
        let keys: BTreeSet<&str> = schema["properties"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            BTreeSet::from([
                "sandbox_id",
                "cmd",
                "args",
                "argv",
                "stdin",
                "env",
                "timeout_ms",
                "workdir"
            ])
        );
        let workdir = schema["properties"]["workdir"]["description"]
            .as_str()
            .unwrap();
        assert!(workdir.contains("`/` when omitted"), "{workdir}");
    }
}
