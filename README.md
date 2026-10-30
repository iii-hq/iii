# Evidence: `requested`, `target` and `targets` in compose::status

Media for the pull request that reports each container's declared selector and the host's targets in `compose::status` and `compose::list`. Orphan branch, media only: delete it after the merge.

Recorded on 2026-10-29 on x86_64 Linux. One project (`/tmp/compose-demo/project`, engine on `127.0.0.1:49349`) under the released iii 0.25.0 and under the branch build (`d72d4c3a0`), on one lock, with the packages in a local cache. The branch build carried a test-only patch, not part of the pull request, that sends `/resolve` to a local registry; every HTTPS request went to a dead proxy.

| File | What it shows |
|---|---|
| `compose-status-transcript.txt` | The whole session, exact: the project, both builds' `compose::status` and `compose::list`, the no-restart update, the empty version |
| `compose-project.png` | The project: `state` pinned to `0.22.23`, `queue` on `latest`, `cron` on `^0.21.26`, a `path://` worker |
| `compose-status-released-0.25.0.png` | iii 0.25.0: `compose::status` with `version` only |
| `compose-status-branch.png` | The branch: `requested` for each package (none for the `path://` worker), `target`, `targets` |
| `compose-list-released-vs-branch.png` | `compose::list` trimmed with `jq`, both builds (`jq` prints `null` for an absent key) |
| `compose-update-no-restart.png` | `compose::update worker=state@latest` on a pin that is also `latest`: nothing restarts (same pids, `total: 0`), `requested` is `latest` at once, in the file and the lock too |
| `compose-empty-version-refused.png` | `version: ""` refused with `MISSING_VERSION_FOR_PACKAGE` |
| `app-installed-updates.mp4` (1m35s) | The iii app (Android) on an engine built from the branch: Installed, what's new, an update with its confirmation and progress, the pin review (pin or follow latest), the review of an update that brings a worker, a stopped and a deprecated worker, the engine's modules |
| `app-installed.png` | Installed: updates that can run now and the two that need a review, from the declared selectors |
| `app-review-pin.png` | The review of a release outside a pin: "Pin 0.21.27" or "Follow latest" |
| `app-up-to-date.png` | Up to date (each worker's version and selector) and the modules built into the engine |
