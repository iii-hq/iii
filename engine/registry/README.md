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

On Windows, in PowerShell (Windows PowerShell 5.1 or PowerShell 7):

```powershell
irm https://iii.dev/registry/install/iii/latest.ps1 | iex
```

It installs `iii.exe` to `~\.local\bin` and adds that folder to your `Path`.
`irm | iex` takes no arguments, so set the options first, for example
`$env:VERSION = '0.27.0'` for another version, `$env:BIN_DIR` for another
folder, or `$env:III_NON_INTERACTIVE = '1'` to skip the setup offer.

Docs: https://iii.dev/docs
