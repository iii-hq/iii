import json
import os
from pathlib import Path

from iii.process_marker import announce_process


def test_marker_names_this_process_until_removed(tmp_path: Path) -> None:
    remove = announce_process("ws://127.0.0.1:49134", "orders", tmp_path)
    [marker] = tmp_path.glob(f"worker-{os.getpid()}-*.json")
    written = json.loads(marker.read_text())
    assert written["pid"] == os.getpid()
    assert written["url"] == "ws://127.0.0.1:49134"
    assert written["name"] == "orders"
    assert written["sdk"] == "python"

    remove()
    assert not marker.exists()


def test_each_client_in_a_process_has_its_own_marker(tmp_path: Path) -> None:
    orders = announce_process("ws://127.0.0.1:49134", "orders", tmp_path)
    billing = announce_process("ws://127.0.0.1:49134", "billing", tmp_path)
    orders()
    assert [json.loads(left.read_text())["name"] for left in tmp_path.iterdir()] == ["billing"]
    billing()


def test_unwritable_directory_is_not_an_error(tmp_path: Path) -> None:
    blocker = tmp_path / "file"
    blocker.write_text("")
    announce_process("ws://x", "y", blocker / "run")()
