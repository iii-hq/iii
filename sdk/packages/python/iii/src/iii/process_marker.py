"""``~/.iii/run/worker-<pid>-<n>.json``: how ``iii ps`` finds a worker however
it was started, one file per client. Compose already marks its children
through their environment; this covers a worker run straight from a terminal.

Best effort: a home directory that cannot be written only makes the worker
harder to find. A process killed before it removes the marker leaves it
behind, and ``iii ps`` discards any marker whose process is gone.
"""

from __future__ import annotations

import atexit
import itertools
import json
import os
import time
from collections.abc import Callable
from pathlib import Path

_announced = itertools.count(1)


def announce_process(url: str, name: str, directory: Path | None = None) -> Callable[[], None]:
    """Write this process's marker and return a function that removes it."""
    try:
        directory = directory or Path.home() / ".iii" / "run"
        marker = directory / f"worker-{os.getpid()}-{next(_announced)}.json"
        directory.mkdir(parents=True, exist_ok=True)
        marker.write_text(
            json.dumps(
                {
                    "pid": os.getpid(),
                    "url": url,
                    "name": name,
                    "sdk": "python",
                    "started_at": int(time.time() * 1000),
                }
            )
        )
    except Exception:
        return lambda: None

    def remove() -> None:
        atexit.unregister(remove)
        marker.unlink(missing_ok=True)

    atexit.register(remove)
    return remove
