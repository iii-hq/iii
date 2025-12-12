"""Bridge instance for Motia framework."""

import os

from iii import Bridge

bridge = Bridge(os.environ.get("III_BRIDGE_URL", "ws://localhost:49134"))
