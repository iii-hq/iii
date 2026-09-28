"""Test SDK fixture preparation without executing the iii CLI."""

import os
from pathlib import Path
import signal
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]


class FixtureStartTests(unittest.TestCase):
    def test_isolated_configuration_and_http_readiness(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            config = root / "test.yaml"
            config.write_text("engine: {workers: {}}\ncontainers: {}\n")
            fixture = root / "test.configuration"
            fixture.mkdir()
            source = "id: http\nname: HTTP\ndescription: test\nvalue: {port: 3199}\n"
            (fixture / "http.yaml").write_text(source)
            (fixture / "ready-port").write_text("3199\n")
            fake = root / "fake-engine"
            fake.write_text(
                '#!/bin/bash\n'
                'printf "%s" "$III_SDK_CONFIGURATION_DIR" > "$CAPTURE"\n'
                'printf "up: 1 of 1 changed\\n"\n'
                'while :; do sleep 0.1; done\n'
            )
            fake.chmod(0o700)
            nc = root / "nc"
            nc.write_text('#!/bin/bash\nprintf "%s\\n" "$*" >> "$PROBES"\nexit "$PROBE_STATUS"\n')
            nc.chmod(0o700)
            copies = []
            for probe_status in ["0", "0", "1"]:
                capture = root / "capture"
                probes = root / "probes"
                pid = root / "pid"
                env = dict(os.environ, TMPDIR=str(root), CAPTURE=str(capture),
                           PROBES=str(probes), PROBE_STATUS=probe_status,
                           PATH=f"{root}:{os.environ['PATH']}")
                result = subprocess.run(
                    ["bash", str(ROOT / "scripts/start-iii.sh"),
                     "--binary", str(fake), "--config", str(config),
                     "--port", "49199", "--pid-file", str(pid),
                     "--log-file", str(root / "engine.log"), "--timeout", "3"],
                    env=env, capture_output=True, text=True, timeout=25,
                )
                self.assertEqual(result.returncode == 0, probe_status == "0", result.stdout + result.stderr)
                copied = Path(capture.read_text())
                copies.append(copied)
                self.assertIn("-z 127.0.0.1 3199", probes.read_text())
                if probe_status == "0":
                    try:
                        self.assertEqual((copied / "http.yaml").read_text(), source)
                        (copied / "http.yaml").write_text("changed by worker")
                        self.assertEqual((fixture / "http.yaml").read_text(), source)
                    finally:
                        os.kill(int(pid.read_text()), signal.SIGTERM)
                else:
                    self.assertFalse(pid.exists())
                    self.assertFalse(copied.exists())
            self.assertEqual(len(set(copies)), 3)


if __name__ == "__main__":
    unittest.main()
