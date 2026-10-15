# iii-console (removed)

The legacy `iii-console` binary has been removed. Use the iii console in ADE instead:

```bash
# in your iii project, with the engine running (`iii compose --up`)
iii trigger compose::add worker=ade
# then open http://127.0.0.1:3113
```

See [Console](https://iii.dev/docs/using-iii/console).

`packages/console-rust` now builds only a placeholder `iii-console` binary. It prints the notice
above and exits with a non-zero status on any invocation. It is still released so that older `iii`
CLIs (`iii update`, `iii console`) find an `iii-console` asset. It will stop shipping in an upcoming
release.
