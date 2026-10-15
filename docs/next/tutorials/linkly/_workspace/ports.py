#!/usr/bin/env python3
"""Move a scaffolded Linkly project off the tutorial's default ports.

    ports.py <project> <engine-port> <http-port> <browser-port>

Rewrites worker-compose.yaml in place: the engine URL (also the one rbac-proxy
dials), the http worker's port and the rbac-proxy listener port. With the
defaults (49134, 3111, 3110) the file is left unchanged.
"""

import pathlib
import re
import sys


def main() -> int:
    project = pathlib.Path(sys.argv[1])
    engine, http, browser = (int(value) for value in sys.argv[2:5])
    path = project / "worker-compose.yaml"
    text = path.read_text()

    text = text.replace("ws://127.0.0.1:49134", f"ws://127.0.0.1:{engine}")
    # rbac-proxy's own listener, in its config_override (commented or not).
    text = re.sub(r"^(\s*#?\s*port:) 3110$", rf"\g<1> {browser}", text, flags=re.M)
    if http != 3111:
        text, count = re.subn(
            r"^(  http:\n    worker: package://http\n(?:    .*\n)*?    config_name: http\n)",
            rf"\g<1>    config_override:\n      port: {http}\n",
            text,
            flags=re.M,
        )
        if count != 1:
            print("error: http container block not found in worker-compose.yaml", file=sys.stderr)
            return 1

    path.write_text(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
