# SDK integration configuration

Start these fixtures through `scripts/start-iii.sh`. For a fixture named
`config-test.yaml`, the launcher copies `config-test.configuration/*.yaml` to a
fresh private temporary directory and exports `III_SDK_CONFIGURATION_DIR` to the
child engine. Each engine instance receives its own mutable copy; tracked fixture
files are never used as the writable configuration store.

These entries are test base configuration, not Compose runtime overrides. The
pinned released workers read configuration from the service. Compose injects
runtime overrides into that service without persisting them; no snapshot file
is delivered. SDK tests exercise SDK behavior, while Compose tests cover the
execution configuration and its separation from the persistent base.

An optional `ready-port` companion file requires that TCP port on 127.0.0.1 to be
reachable after Compose reports startup success. The main SDK fixture requires
3199, preventing a worker registered on the wrong port from passing readiness.
Failed startup removes its temporary configuration directory. Successful runs
retain it for diagnostics in the temporary directory printed by the launcher;
CI runner teardown removes it. For a manual successful run, stop the engine before
removing that printed directory.

## Workers built from source

`workers-ref` holds the `iii-hq/workers` commit that the SDK CI jobs build the
`state`, `http`, `pubsub`, `cron` and `queue` workers from.
`scripts/ci/sdk-test-workers.sh build` fetches that commit and builds the five
workers against this checkout's Rust SDK crates through a `[patch.crates-io]`
override. `scripts/ci/sdk-test-workers.sh fixtures` then writes a variant of
every fixture in this directory: each `package://api.workers.iii.dev/<worker>`
container becomes a `path://` container that runs the built binary, and the
engine section and companion configuration directory stay as committed. The
SDK jobs in `.github/workflows/ci.yml` start these variants.

Locally (the outputs go to `target/sdk-test-workers`, or to
`$SDK_TEST_WORKERS_DIR` when set):

```bash
bash scripts/ci/sdk-test-workers.sh build
bash scripts/ci/sdk-test-workers.sh fixtures
bash scripts/start-iii.sh \
  --config target/sdk-test-workers/fixtures/config-test.yaml --port 49199
```

The committed fixtures start the released worker packages they name. To test
against another workers commit, write its SHA to `workers-ref`.
