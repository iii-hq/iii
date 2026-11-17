# iii

The iii engine and its CLI, in one binary. Workers connect to it over
WebSocket, register functions, and observe each other's functions.

## Install

```sh
curl -fsSL https://iii.dev/registry/install/iii/latest.sh | sh
```

Replace `latest` with a release tag (`rc`, `next`, `alpha`) or an exact
version (`0.27.0`) to pick another one. Add `-s -- --help` after `sh` to see
the options.

The installer checks the archive against the SHA-256 the registry pinned for
that version, installs `iii` to `~/.local/bin` (`BIN_DIR` or `PREFIX` change
it), and offers to set up a first project.

Docs: https://iii.dev/docs
