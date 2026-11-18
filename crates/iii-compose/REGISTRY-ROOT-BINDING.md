# Registry requested-root binding v1

## Scope and status

Isolated consumer change on `c5129b8da5c0f4db8719ae6dd58aee6fa297bc9b`.
This is a proposed Registry `/resolve` extension, not an assertion that deployed
registries already implement it. No SDK, adapter, WebSocket, lock schema, package
archive, dependency version, or engine artifact changes.

The scenario17 requirement at smoke commit `e23b4aa` is unchanged: send effective
references and Linux targets; preserve raw YAML references, escapes and selectors.
Diagnosis: smoke `3787a5fb229d957fbfed123fbc41d871ca0b93f7`,
`17-image-and-interpolation/LITERAL-RESOLVE-GRAPH-FIX.md`.

## Why existing fields cannot prove this binding

Base source anchors (line numbers at c5129b8):

- `registry.rs:68-103`: `ResolveResponse` contains `graph`/`edges`; a worker has
  `name`, optional `alias_of`, and immutable release/artifact metadata.
- `registry.rs:1609-1616,1694-1717`: all graph names, versions and aliases pass
  `check_names`; `is_path_safe` at 1673-1682 intentionally excludes `/`, `$`, braces.
- `registry.rs:1743-1754`: exact resolution requires `worker.name == request`.
- `dependencies.rs:282-289`: planning instead guessed a basename/container key.
- Scenario17 producer `registry_helper.py:111-129` explicitly allowlists
  `${SCENARIO_LITERAL}/tool` as canonical `tool`; `fixture_data.py:47-64` returns
  safe graph names and edges only. Its response does not communicate that mapping.
- No production Registry server implementation is in this engine checkout, and
  no installed Registry source was identified in the local worker tree. The
  engine's worker registry is function registration, not this HTTP package service.

Neither a safe `alias_of`, a unique graph source, the first node, nor an operator
container key proves which opaque request the server resolved. Consequently this
change does not reinterpret those fields, relax path checks or infer a basename.

## Optional wire extension

The existing request remains exactly `{worker, version, target, iii_version?}`.
The selected registry endpoint is still the authority. A response may add:

```json
{
  "root": {
    "schema": 1,
    "requested": {"worker": "${SCENARIO_LITERAL}/tool", "version": "next"},
    "name": "tool",
    "version": "1.2.3"
  },
  "graph": [
    {"name": "queue", "version": "1.2.3", "type": "bundle", "archive_url": "https://registry.example/queue.tar.gz", "sha256": "<64 hex digits>"},
    {"name": "tool", "version": "1.2.3", "type": "bundle", "archive_url": "https://registry.example/tool.tar.gz", "sha256": "<64 hex digits>"}
  ],
  "edges": [{"from": "tool", "to": "queue"}]
}
```

The digest placeholders above are illustrative, not valid install metadata.

`requested.worker` and `requested.version` must echo the two request strings
byte-for-byte. No interpolation, prefix stripping, URL decoding or normalization
is performed. The worker is the portion after the registry's first slash, not a
full URL or another authority. The binding cannot override the authority or target.
An exact-resolution POST for `1.2.3` needs a response echoing `1.2.3`, not `next`.

`name` must identify exactly one safe **graph node**, and `version` must equal that
node's selected version. The node may retain an existing safe alias: `alias_of`
continues to determine canonical package/cache identity. Every graph name is
unique. A competing node whose name equals the request cannot be redirected to
another root. Exact semantic versions must match exactly; parseable ranges use the
existing planner's prerelease-as-release semantics. Tags remain registry-owned.

An explicit binding's graph must be closed, root-reachable and acyclic. Edges use
safe graph names, not request text or foreign registry-qualified references. Missing
nodes, unrelated nodes, self/cyclic edges and duplicate graph names are refused.
This keeps exact lookup from accepting a bound graph planning would interpret as
another dependency set. Existing planner checks still reject incompatible aliases,
release conflicts and cross-registry container collisions. A declared root key that
collides with another node is refused rather than merging packages.

Both graph and exact resolution call `validate_resolution`. Its validated root
index is converted to `Graph.root` for planning or the selected worker for exact
resolution. `Graph.root` is in-process metadata, not a second wire contract; direct
Rust graph constructors need the new field (`None` retains exact-name selection).
The twelve `daemon.rs` changes are test constructors only. Synthetic planning tests
may explicitly name a root; production graph results always originate in validation.

## Compatibility and failure behavior

- Old normal responders with a unique safe node named exactly like the request
  still work without `root`. Existing safe alias/cache semantics are unchanged.
- Missing or `null` root means legacy exact-name selection. An opaque request with
  no matching safe node is unsupported and fails `PACKAGE_NOT_RESOLVED` with explicit
  binding guidance. There is no single-node, container-key or basename fallback.
- `ResolveResponse` and graph workers retain their existing unknown-field tolerance.
  `root` and `root.requested` are strict (`deny_unknown_fields`). A present malformed
  extension is a deserialization/registry error, not a silent legacy downgrade.
  Unsupported `root.schema` fails closed. Future semantics require a new reviewed
  schema; schema 1 cannot quietly acquire another authority or binding map.
- Older consumers ignore `root` due to their existing serde behavior. This does
  **not** make opaque requests work on old engines. Rollout requires both consumer
  support and an explicitly updated producer; no capability negotiation is added.
- Duplicate names and mismatched selected versions are now rejected even in legacy
  replies; those inconsistent responses are not a supported compatibility case.

## Identity, persistence and security

The request is not a path. Whole-graph `check_names`, `is_path_safe`, registry cache
namespace hashing, canonical `alias_of` cache keys, digest validation, verified
extraction and cache-integrity checks remain unchanged. There is no extra trust in
a Registry's filesystem input. This binding is not a signature: as before, a chosen
Registry supplies package metadata, URLs and hashes; authenticity beyond that
existing trust model is outside this patch. Manifest names are not newly trusted
as request identities and are not required to contain the opaque reference.

YAML keeps the original raw reference (including `${VAR}` and `$${...}`) and explicit
selector. An omitted selector is pinned by the existing planner. The existing lock
schema stores `worker` as the **effective** reference (the literal `${...}` survives,
not its escaped YAML spelling), `requested` as the selector, and `resolved.name` as
`tool` for this binding. Safe aliases keep their existing `resolved.name`/`alias_of`.
Raw spelling belongs to YAML; no duplicate raw-reference lock field is introduced.
No reference is expanded a second time between planning and exact resolution.

## Required follow-up, not part of this patch

After contract review, the scenario17 owner must update its allowlisted helper to
emit schema 1 binding metadata for the literal request for both selector and exact
POSTs, retaining canonical tool/queue graph names, artifact bytes/hashes, manifests,
SDK locks and P1/P2 evidence. Generic unknown registries lacking this extension are
unsupported for opaque roots, gracefully and explicitly. Normal producers require
no cochange. No external worker, remote repository or smoke fixture was modified.

## Regression matrix and verification boundary

`src/registry_root_tests.rs` adds nine narrow tests:

- valid explicit binding (root not first), safe canonical alias, legacy same-name;
- both HTTP lookup paths reject absent/ambiguous binding, wrong request/authority,
  selector/schema/root/version, unsafe name/version/alias/traversal, dangling,
  foreign, opaque, unrelated and cyclic edges;
- wrong exact/range version and competing exact-name root; strict extension serde;
- real `dependencies::plan` -> text upsert -> parse -> exact HTTP lock resolution ->
  archive SHA verification -> install/cache -> lock persistence, in process;
- nine normal/literal/safe-alias x tag/range/unpinned combinations, Linux root
  targets, raw interpolation/escapes/selectors, canonical manifest and digest,
  canonical/alias cache reuse, exactly two source GETs (tool/queue);
- conflicting safe container key, same dependency name at distinct authorities,
  and wrong archive digest without publishing a cache install.

Existing raw-reference planning tests now declare the binding they previously
assumed. Their subprocess environment matrix remains unchanged; the new joined
regression uses a local mock HTTP server and temporary files, no subprocess/VM.

**Compilation and every Rust test execution: PENDING, intentionally not run.**
Formatting/source checks are not runtime proof. Future minimum authorized checks:

```text
cargo test -p iii-compose registry::root_tests
cargo test -p iii-compose explicit_registry_references_remain_raw_after_resolution_and_upsert
cargo test -p iii-compose a_renamed_container_plans_the_package_it_runs
cargo test -p iii-compose registry::tests
cargo test -p iii-compose dependencies::tests
```

No command above was run. Future heavy admission requires separate authorization,
metadata admission, `/tmp/iii-agents-heavy.lock`, nice 15 / ionice c3 / jobs 4 and the
parent's shared target/caps. This patch has runtime=0 and build=0. It neither
reattributes old smoke passes nor changes the pinned final binary `544470`.
