# PR draft: fix(compose): bind opaque registry requests to validated package roots

## Summary

- Add optional, versioned `/resolve` response `root` metadata tying the exact worker
  and selector to one safe graph node/version.
- Use one validator for graph planning and exact package resolution. Remove
  basename/container-key guessing; reject ambiguous or inconsistent bindings.
- Preserve canonical package/cache/alias semantics and raw compose references,
  escapes and selectors. No lock schema, SDK, adapter or artifact changes.
- Add nine focused contract and in-process HTTP/lock/archive/cache regressions.

Base: `c5129b8da5c0f4db8719ae6dd58aee6fa297bc9b`.
Branch: `fix/registry-requested-root-binding`.
Worktree: `/home/doggao/iii/wt/iii.registry-root-binding`.

## Design and rollout

See [REGISTRY-ROOT-BINDING.md](REGISTRY-ROOT-BINDING.md) for wire schema 1,
producer evidence, serde behavior, backward compatibility, failure modes and the
security boundary. This is a proposed consumer contract, not a deployed producer
feature. Normal same-name responders need no changes. Scenario17's literal-root
helper requires a separate reviewed producer update; no smoke file was edited.

Old engines ignore the optional response field and still cannot resolve opaque
roots. New engines refuse opaque requests without a validated explicit binding.
No basename substitution, literal expansion, arbitrary first-node selection,
path-safety relaxation or trust in container names is introduced.

## Verification evidence (source only)

Performed in this worktree, before the local commit:

```text
rustfmt --check --edition 2024 --config skip_children=true \
  crates/iii-compose/src/registry.rs \
  crates/iii-compose/src/dependencies.rs \
  crates/iii-compose/src/daemon.rs \
  crates/iii-compose/src/registry_root_tests.rs
=> exit 0

git diff --check
=> exit 0
```

Source comparison against the exact base, using Git blobs and a local read-only
Python assertion script, passed:

```text
UNCHANGED: is_path_safe, check_names, registry_cache_root, cache_install_dir,
package_name, install_bundle, validated_cache_digest, download_and_extract,
cache_matches, into_resolved_package (10 complete function bodies).
UNCHANGED: daemon production source; exactly 12 test-only Graph initializers.
REMOVED: dependencies basename/container-key root fallback.
```

`rustfmt` was invoked directly because Cargo is expressly prohibited in this task.
It parses/formats source; it does not establish type correctness or test success.
The required report/Rust/GitHub skill texts and repository AGENTS.md were read.
Supplemental Rust handbook chapter reads were rejected by the filesystem scope;
no workaround or outside-scope read was attempted.

## Pending checks, not PASS

All Rust compilation and all nine new tests are **PENDING**. See the contract doc's
minimum test matrix. The local mock server, test archives and temporary caches
exist only as unexecuted Rust test code. There was no network listener, artifact
GET, process/VM startup, Cargo/rustc/build, SDK/npm command or cache inspection.

Runtime runs: **0**. Builds: **0**. New engine artifacts: **0**.
No push, remote PR, merge, tag, release or upstream modification. The existing
final binary pin `544470`, smoke archives/manifests, SDK pins, original failed P3
and P1/P2 receipts are untouched; no prior PASS is attributed to this change.
Chrono stays unchanged; Jiff integration is a separate parent's concern.

## Review / merge gate

Independent review of the new optional Registry contract and source changes is
required before producer work or publication. The parent owns integration and any
remote PR. Heavy compilation/tests require fresh separate authorization and
metadata admission, the parent's shared target, `/tmp/iii-agents-heavy.lock`,
nice 15, ionice c3 and jobs 4. No current capacity or automatic cap increase is
assumed. A future engine artifact and full scenario17 run are separate gated work.
