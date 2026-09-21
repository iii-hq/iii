# SDK integration configuration

Start these fixtures through `scripts/start-iii.sh`. For a fixture named
`config-test.yaml`, the launcher copies `config-test.configuration/*.yaml` to a
fresh private temporary directory and exports `III_SDK_CONFIGURATION_DIR` to the
child engine. Each engine instance receives its own mutable copy; tracked fixture
files are never used as the writable configuration store.

These entries are test base configuration, not Compose runtime overrides. The
pinned released workers read configuration from the service and do not consume
the `III_CONFIG` execution snapshot. SDK tests exercise SDK behavior rather than
worker support for runtime overrides; Compose tests cover non-persistence of
`config_override` separately.

An optional `ready-port` companion file requires that TCP port on 127.0.0.1 to be
reachable after Compose reports startup success. The main SDK fixture requires
3199, preventing a worker registered on the wrong port from passing readiness.
Failed startup removes its temporary configuration directory. Successful runs
retain it for diagnostics in the temporary directory printed by the launcher;
CI runner teardown removes it. For a manual successful run, stop the engine before
removing that printed directory.
