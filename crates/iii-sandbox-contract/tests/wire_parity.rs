use iii_sandbox_contract::{
    AdapterInfo, AdapterInfoRequest, CatalogListRequest, ChannelDirection, ContractVersion,
    DOCS_BASE, EnvShape, ExecRequest, ExecResponse, GrepRequest, ListRequest, LogRecord, LogStream,
    MkdirRequest, OPERATIONS, ReadResponse, RouterInfo, RunResponse, SandboxErrorCode, SedRequest,
    State, StopRequest, StreamChannelRef, WorkerDestroyRequest, WorkerDestroyResponse,
    WorkerLogsRequest, WorkerLogsResponse, WorkerPrepareRequest, WorkerPrepareResponse,
    WorkerSource, WorkerStartRequest, WorkerStartResponse, WorkerStatus, WorkerStatusRequest,
    WorkerStopRequest, WriteContent, WriteRequest, code_of, error_payload,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, from_value, json, to_value};
use std::collections::{BTreeMap, BTreeSet};

use SandboxErrorCode::*;

const FIXTURE: &str = include_str!("fixtures/sandbox_error_codes.json");

#[derive(Deserialize)]
struct Fixture {
    docs_base: String,
    codes: Vec<Row>,
}

#[derive(Deserialize)]
struct Row {
    code: String,
    #[serde(rename = "type")]
    error_type: String,
    retryable: bool,
}

fn fixture() -> Fixture {
    serde_json::from_str(FIXTURE).expect("fixture parses")
}

/// Compile-time exhaustiveness guard: adding a `SandboxErrorCode` variant breaks this
/// match, forcing `ALL` and the fixture to be extended in lockstep.
fn enforce_exhaustive(c: SandboxErrorCode) -> SandboxErrorCode {
    match c {
        S001 | S002 | S003 | S004 | S100 | S101 | S102 | S200 | S210 | S211 | S212 | S213
        | S214 | S215 | S216 | S217 | S218 | S219 | S300 | S301 | S302 | S303 | S304 | S400
        | S501 | S502 | S503 | S504 | S505 | S506 => c,
    }
}

const ALL: [SandboxErrorCode; 30] = [
    S001, S002, S003, S004, S100, S101, S102, S200, S210, S211, S212, S213, S214, S215, S216, S217,
    S218, S219, S300, S301, S302, S303, S304, S400, S501, S502, S503, S504, S505, S506,
];

#[test]
fn fixture_rows_match_error_payload() {
    let f = fixture();
    assert_eq!(f.docs_base, DOCS_BASE);
    assert_eq!(f.codes.len(), 30);
    for row in &f.codes {
        let code: SandboxErrorCode =
            serde_json::from_value(Value::String(row.code.clone())).expect("row code is a variant");
        let p = error_payload(code, "x", None, None);
        assert_eq!(p["type"], row.error_type, "{}", row.code);
        assert_eq!(p["retryable"], row.retryable, "{}", row.code);
        assert_eq!(
            p["docs_url"],
            format!("{DOCS_BASE}{}", row.code),
            "{}",
            row.code
        );
    }
}

#[test]
fn every_code_has_a_fixture_row() {
    let rows: BTreeSet<String> = fixture().codes.into_iter().map(|r| r.code).collect();
    for c in ALL {
        let c = enforce_exhaustive(c);
        assert!(
            rows.contains(c.as_str()),
            "no fixture row for {}",
            c.as_str()
        );
    }
}

#[test]
fn payload_keeps_todays_shape() {
    assert_eq!(
        error_payload(
            S211,
            "parent directory not found: /w",
            Some(json!({"parents": true})),
            Some("merge")
        ),
        json!({"type": "filesystem", "code": "S211", "message": "parent directory not found: /w",
               "docs_url": format!("{DOCS_BASE}S211"), "retryable": false,
               "fix": {"parents": true}, "fix_note": "merge"})
    );
    let p = error_payload(S301, "m", None, None);
    assert_eq!(
        (&p["type"], &p["retryable"], &p["fix"], &p["fix_note"]),
        (
            &json!("transient"),
            &json!(true),
            &Value::Null,
            &Value::Null
        )
    );
}

#[test]
fn code_of_reads_wire_messages() {
    assert_eq!(
        code_of(&error_payload(S504, "x", None, None).to_string()),
        Some(S504)
    );
    // The adapter's errors reach callers through the SDK in this exact form.
    let real = r#"handler error: {"code":"S504","docs_url":"https://iii.dev/docs/creating-workers/sandboxes#S504","fix":null,"fix_note":"the adapter restarted; start the VM again","message":"no VM 4e0be037530c82614343dd49cbd58e61 on this adapter","retryable":false,"type":"validation"}"#;
    assert_eq!(code_of(real), Some(S504));
    for m in [
        "not json",
        "handler error: boom",
        r#"{"code":"S999"}"#,
        r#"{"message":"x"}"#,
    ] {
        assert_eq!(code_of(m), None);
    }
}

#[test]
fn every_code_has_a_docs_anchor() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/next/creating-workers/sandboxes.mdx"
    );
    let page = std::fs::read_to_string(path).expect("sandboxes.mdx readable");
    for c in ALL {
        let anchor = format!("<a id=\"{}\"></a>", c.as_str());
        assert!(page.contains(&anchor), "sandboxes.mdx lacks {anchor}");
    }
}

fn stream_ref() -> StreamChannelRef {
    StreamChannelRef {
        channel_id: "c".into(),
        access_key: "k".into(),
        direction: ChannelDirection::Read,
    }
}

#[test]
fn exec_accepts_todays_three_shapes() {
    let exec = |v: Value| from_value::<ExecRequest>(v).expect("exec request parses");

    let line = exec(json!({"sandbox_id": "s", "cmd": "node -v"}));
    assert_eq!(line.cmd, "node -v");
    assert!(line.args.is_empty());

    let classic = exec(json!({"sandbox_id": "s", "cmd": "node", "args": ["-v"]}));
    assert_eq!(classic.cmd, "node");
    assert_eq!(classic.args, ["-v"]);

    let argv = exec(json!({"sandbox_id": "s", "argv": ["node", "-v"], "stdin": "aGk="}));
    assert_eq!(argv.argv, ["node", "-v"]);
    assert_eq!(argv.cmd, "");
    assert_eq!(argv.stdin.as_deref(), Some("aGk="));
}

#[test]
fn env_accepts_list_and_map() {
    let env = |v: Value| from_value::<EnvShape>(v).expect("env parses").into_kv_vec();
    assert_eq!(env(json!(["A=1"])), Ok(vec!["A=1".to_string()]));
    assert_eq!(
        env(json!({"B": "2", "A": "1"})),
        Ok(vec!["A=1".to_string(), "B=2".to_string()])
    );
}

#[test]
fn env_map_rejects_bad_names_with_todays_text() {
    assert_eq!(
        EnvShape::Map(BTreeMap::from([("9bad".into(), "x".into())]))
            .into_kv_vec()
            .unwrap_err(),
        "env contains invalid variable name(s): [\"9bad\"]. Names must match `[A-Za-z_][A-Za-z0-9_]*`."
    );
}

#[test]
fn requests_ignore_unknown_fields() {
    from_value::<ListRequest>(json!({"show_all": true})).expect("ListRequest");
    from_value::<CatalogListRequest>(json!({"x": 1})).expect("CatalogListRequest");
    from_value::<StopRequest>(json!({"sandbox_id": "s", "extra": 1})).expect("StopRequest");
}

#[test]
fn request_defaults_match_daemon() {
    let grep: GrepRequest =
        from_value(json!({"sandbox_id": "s", "path": "/", "pattern": "p"})).unwrap();
    assert_eq!(
        (grep.recursive, grep.max_matches, grep.max_line_bytes),
        (true, 10_000, 4096)
    );

    let sed: SedRequest =
        from_value(json!({"sandbox_id": "s", "pattern": "a", "replacement": "b"})).unwrap();
    assert!(sed.recursive && sed.regex && sed.files.is_empty());

    let mkdir: MkdirRequest = from_value(json!({"sandbox_id": "s", "path": "/d"})).unwrap();
    assert_eq!(mkdir.mode, "0755");

    let write: WriteRequest = from_value(json!({"sandbox_id": "s", "path": "/f"})).unwrap();
    assert_eq!(write.mode, "0644");
    assert!(write.content.is_none());

    let stop: StopRequest = from_value(json!({"sandbox_id": "s"})).unwrap();
    assert!(!stop.wait);
}

#[test]
fn write_content_is_untagged() {
    assert!(matches!(from_value(json!("hi")), Ok(WriteContent::Utf8(s)) if s == "hi"));
    assert!(matches!(
        from_value(json!({"channel_id": "c", "access_key": "k", "direction": "read"})),
        Ok(WriteContent::Stream(_))
    ));
}

#[test]
fn exec_response_keeps_null_exit_code() {
    let v = to_value(ExecResponse {
        stdout: String::new(),
        stderr: String::new(),
        exit_code: None,
        timed_out: true,
        duration_ms: 1,
        success: false,
        stdout_truncated: false,
        stderr_truncated: false,
    })
    .unwrap();
    assert_eq!(v.get("exit_code"), Some(&Value::Null));
}

#[test]
fn exec_reply_truncation_flags_default_false() {
    // A contract 1.0 adapter's reply has no truncation flags.
    let old = json!({"stdout": "", "stderr": "", "exit_code": 0, "timed_out": false, "duration_ms": 1, "success": true});
    let exec: ExecResponse = from_value(old.clone()).unwrap();
    assert!(!exec.stdout_truncated && !exec.stderr_truncated);
    let run: RunResponse = from_value(old).unwrap();
    assert!(!run.stdout_truncated && !run.stderr_truncated);

    let v = to_value(ExecResponse {
        stdout_truncated: true,
        ..exec
    })
    .unwrap();
    assert_eq!(v["stdout_truncated"], json!(true));
    assert_eq!(v["stderr_truncated"], json!(false));
}

#[test]
fn optional_reply_fields_stay_omitted() {
    let read = to_value(ReadResponse {
        content: stream_ref(),
        body: None,
        size: 0,
        mode: "0644".into(),
        mtime: 0,
    })
    .unwrap();
    assert!(read.get("body").is_none());

    let run = to_value(RunResponse {
        stdout: String::new(),
        stderr: String::new(),
        exit_code: Some(0),
        timed_out: false,
        duration_ms: 1,
        success: true,
        stdout_truncated: false,
        stderr_truncated: false,
        sandbox_id: None,
    })
    .unwrap();
    assert!(run.get("sandbox_id").is_none());
}

/// Serialise `t`, parse it back as `T`, and require the second serialisation to match the first.
fn assert_parses_back<T: Serialize + DeserializeOwned>(t: &T) -> Value {
    let first = to_value(t).unwrap();
    let again: T = from_value(first.clone()).expect("serialised request parses back");
    assert_eq!(to_value(&again).unwrap(), first);
    first
}

#[test]
fn serialized_requests_parse_back() {
    let exec = assert_parses_back(&ExecRequest {
        sandbox_id: "s".into(),
        cmd: String::new(),
        args: vec![],
        argv: vec!["node".into(), "-v".into()],
        stdin: None,
        env: EnvShape::Map(BTreeMap::from([("A".into(), "1".into())])),
        timeout_ms: None,
        workdir: None,
    });
    assert_eq!(exec["env"], json!({"A": "1"}));

    assert_parses_back(&WriteRequest {
        sandbox_id: "s".into(),
        path: "/f".into(),
        mode: "0644".into(),
        parents: false,
        content: Some(WriteContent::Stream(stream_ref())),
        content_b64: None,
    });

    assert_parses_back(&SedRequest {
        sandbox_id: "s".into(),
        files: vec![],
        path: Some("/w".into()),
        recursive: true,
        include_glob: vec![],
        exclude_glob: vec![],
        pattern: "a".into(),
        replacement: "b".into(),
        regex: true,
        first_only: false,
        ignore_case: false,
    });
}

// ---------------------------------------------------------------------------
// adapter info and worker-VM family
// ---------------------------------------------------------------------------

#[test]
fn router_info_unbound_json() {
    let unbound = RouterInfo {
        adapter: None,
        namespace: None,
        contract: ContractVersion::CURRENT,
        info: None,
    };
    assert_eq!(
        to_value(unbound).unwrap(),
        json!({"adapter": null, "namespace": null, "contract": {"major": 1, "minor": 1}, "info": null})
    );
}

#[test]
fn adapter_info_json() {
    let v = json!({
        "adapter": "sandbox-libkrun",
        "adapter_version": "1.0.0",
        "contract": {"major": 1, "minor": 0}
    });
    let info: AdapterInfo = from_value(v.clone()).unwrap();
    assert_eq!(info.contract.major, 1);
    assert_eq!(to_value(&info).unwrap(), v);
    from_value::<AdapterInfoRequest>(json!({})).expect("empty request parses");
}

#[test]
fn worker_source_is_kind_tagged() {
    let bundle = WorkerSource::Bundle {
        package: "my-bundle".into(),
        host_path: "/c".into(),
    };
    assert_eq!(
        to_value(bundle).unwrap(),
        json!({"kind": "bundle", "package": "my-bundle", "host_path": "/c"})
    );
    let local = WorkerSource::Local {
        host_path: "/p".into(),
        run_override: None,
    };
    assert_eq!(
        to_value(local).unwrap(),
        json!({"kind": "local", "host_path": "/p", "run_override": null})
    );
}

#[test]
fn worker_status_and_log_record_json() {
    let status = WorkerStatus {
        vm_id: "vm".into(),
        instance: 2,
        state: State::Exited,
        exit_code: None,
        signal: Some(9),
        started_at_ms: 10,
        ended_at_ms: Some(20),
    };
    let v = to_value(status).unwrap();
    assert_eq!(v["state"], "exited");
    assert_eq!(v["exit_code"], Value::Null);
    assert_eq!(v["signal"], 9);

    let record = LogRecord {
        stream: LogStream::Stderr,
        line: "x".into(),
        ts_ms: 1,
        cursor: 7,
    };
    assert_eq!(
        to_value(record).unwrap(),
        json!({"stream": "stderr", "line": "x", "ts_ms": 1, "cursor": 7})
    );
}

#[test]
fn worker_start_request_round_trips() {
    let v = json!({
        "auth": "tok",
        "state_key": "k",
        "worker_name": "my-bundle",
        "source": {"kind": "bundle", "package": "my-bundle", "host_path": "/c"},
        "env": {"A": "1"},
        "engine_url": "ws://127.0.0.1:49134"
    });
    let req: WorkerStartRequest = from_value(v.clone()).unwrap();
    assert_eq!(to_value(req).unwrap(), v);
}

fn round_trips<T: Serialize + DeserializeOwned>(name: &str, example: &Value) {
    let t: T = from_value(example.clone()).unwrap_or_else(|e| panic!("{name}: {e}"));
    assert_eq!(&to_value(t).unwrap(), example, "{name}");
}

/// Maps a fixture key to its type by name; a key with no arm fails the test.
macro_rules! round_trip_by_name {
    ($name:expr, $example:expr, $($ty:ident),+ $(,)?) => {
        match $name {
            $(stringify!($ty) => round_trips::<$ty>($name, $example),)+
            other => panic!("no round-trip arm for fixture key {other}"),
        }
    };
}

#[test]
fn wire_examples_round_trip() {
    let examples: BTreeMap<String, Value> =
        serde_json::from_str(include_str!("fixtures/wire_examples.json")).expect("fixture parses");

    // Types declared in info.rs and worker.rs; the fixture needs one example for each.
    let declared: BTreeSet<String> = [
        include_str!("../src/info.rs"),
        include_str!("../src/worker.rs"),
    ]
    .into_iter()
    .flat_map(str::lines)
    .filter_map(|l| {
        l.strip_prefix("pub struct ")
            .or_else(|| l.strip_prefix("pub enum "))
    })
    .map(|rest| rest.chars().take_while(|c| c.is_alphanumeric()).collect())
    .collect();
    assert_eq!(examples.keys().cloned().collect::<BTreeSet<_>>(), declared);

    for (name, example) in &examples {
        round_trip_by_name!(
            name.as_str(),
            example,
            ContractVersion,
            AdapterInfoRequest,
            AdapterInfo,
            RouterInfo,
            WorkerPrepareRequest,
            WorkerPrepareResponse,
            WorkerStartRequest,
            WorkerSource,
            WorkerStartResponse,
            WorkerStatusRequest,
            WorkerStatus,
            State,
            WorkerLogsRequest,
            WorkerLogsResponse,
            LogRecord,
            LogStream,
            WorkerStopRequest,
            WorkerDestroyRequest,
            WorkerDestroyResponse,
        );
    }
}

// ---------------------------------------------------------------------------
// ephemeral operations
// ---------------------------------------------------------------------------

/// Drops what may differ between the contract's types and an adapter's copies without changing
/// how either side reads a payload: prose, the `[]`/`null` defaults schemars only emits for
/// `Serialize` field types, and a response's `default` (only the adapter writes responses). A
/// response keeps `required`: it lists the fields this crate cannot read a reply without, and the
/// adapter's `tests/wire_examples.rs` checks its serialized replies against it instead of against
/// its own schemas, which `skip_serializing_if` does not change.
fn wire_shape(v: &mut Value, response: bool) {
    match v {
        Value::Array(a) => a.iter_mut().for_each(|v| wire_shape(v, response)),
        Value::Object(m) => {
            m.remove("description");
            if response
                || m.get("default")
                    .is_some_and(|d| d.is_null() || d == &json!([]))
            {
                m.remove("default");
            }
            for (k, v) in m.iter_mut() {
                // These two maps are keyed by field and type names, not schema keywords.
                if k == "properties" || k == "definitions" {
                    v.as_object_mut()
                        .into_iter()
                        .flat_map(|m| m.values_mut())
                        .for_each(|v| wire_shape(v, response));
                } else {
                    wire_shape(v, response);
                }
            }
        }
        _ => {}
    }
}

/// Pins the wire shape of every `sandbox::*` request and response, as the router publishes it,
/// to `fixtures/operation_schemas.json`; the adapter compares its own copies to the same file.
#[test]
fn operation_schemas_match_fixture() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/operation_schemas.json"
    );
    let shapes = Value::Object(
        OPERATIONS
            .iter()
            .map(|op| {
                let (mut req, mut res) = ((op.request_schema)(), (op.response_schema)());
                wire_shape(&mut req, false);
                wire_shape(&mut res, true);
                (
                    op.suffix.to_string(),
                    json!({"request": req, "response": res}),
                )
            })
            .collect(),
    );
    if std::env::var_os("UPDATE_GOLDENS").is_some() {
        std::fs::write(path, serde_json::to_string_pretty(&shapes).unwrap() + "\n").unwrap();
    }
    let fixture: Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("fixture readable")).unwrap();
    assert_eq!(
        fixture, shapes,
        "run UPDATE_GOLDENS=1 cargo test -p iii-sandbox-contract, review the diff and copy the \
         file to the adapter's tests/fixtures"
    );
}
