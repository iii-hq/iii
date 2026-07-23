# worker-compose.yaml v1

One file describes one project's workers. One daemon binds to one file for its
whole lifetime; the daemon's `--id` is how remote control addresses it
(`iii trigger compose::up id=compose-hostb`).

## Canonical shape

```yaml
name: orders

containers:
  database:
    worker: package://workers.iii.dev/database
    version: 1.4.2
    config_name: orders-database
    scripts:
      post_run: "./backup-on-exit.sh"

  api:
    worker: path://./workers/api        # no iii.worker.yaml needed — run: is enough
    depends_on: [database]
    config_name: orders-api
    config_override:
      server:
        port: 3000
    scripts:
      pre_start: "npx prisma migrate deploy"
      pre_start_timeout: 120s
      run: "pnpm dev"
      post_run: "./cleanup-tmp.sh"
```

## Container fields

| Field | Required | Rule |
| --- | --- | --- |
| `worker` | yes | `package://` (registry) or `path://` (local directory) |
| `version` | package only | exact or range, resolved into the lockfile |
| `depends_on` | no | container ids **from the same file only** |
| `config_name` / `config_uri` | no | where the daemon fetches base config (see configuration.md) |
| `config_override` | no | sparse map merged over the fetched base |
| `scripts` | no (`run` required for manifest-less `path://`) | see scripts.md |
| `working_dir` | no | default: worker dir for `path://`, compose-file dir for `package://` |

## Identity

- `containers` is an **id-keyed object**; the key is the worker's lifecycle id
  and its registered name.
- For `path://` with a manifest, the key must equal the manifest `name`. For
  manifest-less `path://` (compose declares `run`), the key **is** the
  identity.
- No key order semantics: start order comes only from the `depends_on` DAG.

## Dependency scope

`depends_on` resolves inside the same file, full stop. Two compose files that
want to share one database do it by **namespace** (both point at the same
namespace, one of them owns the process — see namespace.md), never by a
cross-file dependency edge. This keeps ownership, rollback, and teardown
decidable by one daemon looking at one file.

## Validation (hard errors)

- empty `containers`; unknown or self `depends_on`; dependency cycle (error
  prints the full path: `api -> queue -> database -> api`);
- unknown field anywhere (strict schema);
- `run` on a `package://` worker; `pre_start_timeout` without `pre_start`;
- `path://` with neither manifest nor `run`;
- key ≠ manifest `name` when a manifest exists.

`iii compose validate` runs the whole ruleset offline — no engine required.

## Not in v1

Cross-file `depends_on`, docker runtime keys (`ports`, `image`), full inline
configuration bodies, hot reload, `environment`/`env_file` (children get the
standard injected contract plus the host env they inherit; per-container env
maps return if a concrete need appears).
