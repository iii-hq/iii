"""Unit tests for bump_manifests.py.

Run with: python -m pytest .github/scripts/test_bump_manifests.py -v
"""
from __future__ import annotations

import subprocess
import sys
import textwrap
from pathlib import Path

import pytest

from bump_manifests import bump_cargo_package_version
from bump_manifests import bump_cargo_workspace_dep_version
from bump_manifests import bump_json_top_level_version
from bump_manifests import bump_pep440_dep_pin
from bump_manifests import rewrite_all


class TestBumpCargoPackageVersion:
    def test_replaces_first_version_only(self):
        src = textwrap.dedent(
            """\
            [package]
            name = "iii-observability"
            version = "0.13.0-next.1"
            edition = "2024"

            [dependencies]
            other = { version = "1.2.3" }
            """
        )
        out = bump_cargo_package_version(src, "0.16.0-next.2")
        assert 'version = "0.16.0-next.2"' in out
        assert 'other = { version = "1.2.3" }' in out
        # First match anchored on line start only — no other top-level
        # version line should change.
        assert out.count('version = "0.16.0-next.2"') == 1

    def test_raises_when_no_version(self):
        src = '[package]\nname = "foo"\n'
        with pytest.raises(ValueError, match="no top-level version"):
            bump_cargo_package_version(src, "0.16.0-next.2")


class TestBumpCargoWorkspaceDepVersion:
    def test_replaces_version_pin(self):
        src = (
            "[workspace.dependencies]\n"
            'iii-observability = { path = "sdk/packages/rust/observability", version = "0.13.0-next.1" }\n'
            'tokio = { version = "1", features = ["macros"] }\n'
        )
        out = bump_cargo_workspace_dep_version(src, "iii-observability", "0.16.0-next.2")
        assert (
            'iii-observability = { path = "sdk/packages/rust/observability", version = "0.16.0-next.2" }'
            in out
        )
        assert 'tokio = { version = "1", features = ["macros"] }' in out

    def test_raises_when_dep_missing(self):
        src = '[workspace.dependencies]\ntokio = { version = "1" }\n'
        with pytest.raises(ValueError, match="iii-observability"):
            bump_cargo_workspace_dep_version(src, "iii-observability", "0.16.0-next.2")


class TestBumpJsonTopLevelVersion:
    def test_replaces_first_version(self):
        src = (
            '{\n'
            '  "name": "iii-sdk",\n'
            '  "version": "0.13.0-next.1",\n'
            '  "dependencies": {\n'
            '    "@iii-dev/observability": "workspace:*"\n'
            '  }\n'
            '}\n'
        )
        out = bump_json_top_level_version(src, "0.16.0-next.2")
        assert '"version": "0.16.0-next.2"' in out
        assert '"@iii-dev/observability": "workspace:*"' in out

    def test_raises_when_no_top_level_version(self):
        src = '{ "name": "x" }\n'
        with pytest.raises(ValueError, match="no top-level version"):
            bump_json_top_level_version(src, "1.0.0")


class TestBumpPep440DepPin:
    def test_replaces_pin(self):
        src = (
            'dependencies = [\n'
            '    "websockets>=12.0",\n'
            '    "iii-observability==0.13.0.dev1",\n'
            ']\n'
        )
        out = bump_pep440_dep_pin(src, "iii-observability", "0.16.0.dev2")
        assert '"iii-observability==0.16.0.dev2"' in out
        assert '"websockets>=12.0"' in out

    def test_raises_when_dep_missing(self):
        src = 'dependencies = [\n    "websockets>=12.0",\n]\n'
        with pytest.raises(ValueError, match="iii-observability"):
            bump_pep440_dep_pin(src, "iii-observability", "0.16.0.dev2")


def _write(p: Path, body: str) -> None:
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(body)


def test_rewrite_all_updates_every_target_file(tmp_path: Path):
    root = tmp_path

    _write(root / "Cargo.toml", (
        '[workspace.package]\n'
        'version = "0.15.0-next.1"\n\n'
        '[workspace.dependencies]\n'
        'iii-observability = { path = "sdk/packages/rust/observability", version = "0.13.0-next.1" }\n'
    ))
    _write(root / "engine" / "Cargo.toml", '[package]\nname = "iii"\nversion = "0.15.0-next.1"\n')
    _write(root / "sdk/packages/rust/iii/Cargo.toml", '[package]\nname = "iii-sdk"\nversion = "0.15.0-next.1"\n')
    _write(root / "sdk/packages/rust/observability/Cargo.toml", '[package]\nname = "iii-observability"\nversion = "0.13.0-next.1"\n')
    _write(root / "sdk/packages/rust/helpers/Cargo.toml", '[package]\nname = "iii-helpers"\nversion = "0.13.0-next.1"\n')
    _write(root / "sdk/packages/node/iii/package.json", '{\n  "name": "iii-sdk",\n  "version": "0.15.0-next.1"\n}\n')
    _write(root / "sdk/packages/node/iii-browser/package.json", '{\n  "name": "iii-browser",\n  "version": "0.15.0-next.1"\n}\n')
    _write(root / "sdk/packages/node/observability/package.json", '{\n  "name": "@iii-dev/observability",\n  "version": "0.13.0-next.1"\n}\n')
    _write(root / "sdk/packages/node/helpers/package.json", '{\n  "name": "@iii-dev/helpers",\n  "version": "0.13.0-next.1"\n}\n')
    _write(root / "sdk/packages/python/iii/pyproject.toml", (
        '[project]\nname = "iii-sdk"\nversion = "0.15.0.dev1"\n'
        'dependencies = [\n    "iii-observability==0.13.0.dev1",\n]\n'
    ))
    _write(root / "sdk/packages/python/observability/pyproject.toml", '[project]\nname = "iii-observability"\nversion = "0.13.0.dev1"\n')
    _write(root / "sdk/packages/python/helpers/pyproject.toml", '[project]\nname = "iii-helpers"\nversion = "0.13.0.dev1"\n')
    _write(root / "console/packages/console-rust/Cargo.toml", '[package]\nname = "console-rust"\nversion = "0.15.0-next.1"\n')

    rewrite_all(root=root, new_version="0.16.0-next.2", new_py_version="0.16.0.dev2")

    assert 'version = "0.16.0-next.2"' in (root / "Cargo.toml").read_text()
    assert 'iii-observability = { path = "sdk/packages/rust/observability", version = "0.16.0-next.2" }' in (root / "Cargo.toml").read_text()
    assert 'version = "0.16.0-next.2"' in (root / "engine" / "Cargo.toml").read_text()
    assert 'version = "0.16.0-next.2"' in (root / "sdk/packages/rust/iii/Cargo.toml").read_text()
    assert 'version = "0.16.0-next.2"' in (root / "sdk/packages/rust/observability/Cargo.toml").read_text()
    assert 'version = "0.16.0-next.2"' in (root / "sdk/packages/rust/helpers/Cargo.toml").read_text()
    assert '"version": "0.16.0-next.2"' in (root / "sdk/packages/node/iii/package.json").read_text()
    assert '"version": "0.16.0-next.2"' in (root / "sdk/packages/node/iii-browser/package.json").read_text()
    assert '"version": "0.16.0-next.2"' in (root / "sdk/packages/node/observability/package.json").read_text()
    assert '"version": "0.16.0-next.2"' in (root / "sdk/packages/node/helpers/package.json").read_text()
    py_iii = (root / "sdk/packages/python/iii/pyproject.toml").read_text()
    assert 'version = "0.16.0.dev2"' in py_iii
    assert '"iii-observability==0.16.0.dev2"' in py_iii
    assert 'version = "0.16.0.dev2"' in (root / "sdk/packages/python/observability/pyproject.toml").read_text()
    assert 'version = "0.16.0.dev2"' in (root / "sdk/packages/python/helpers/pyproject.toml").read_text()
    assert 'version = "0.16.0-next.2"' in (root / "console/packages/console-rust/Cargo.toml").read_text()


def test_cli_invokes_rewrite_all(tmp_path: Path):
    root = tmp_path
    _write(root / "Cargo.toml", (
        '[workspace.package]\n'
        'version = "0.15.0-next.1"\n\n'
        '[workspace.dependencies]\n'
        'iii-observability = { path = "sdk/packages/rust/observability", version = "0.13.0-next.1" }\n'
    ))
    _write(root / "engine" / "Cargo.toml", 'version = "0.15.0-next.1"\n')
    _write(root / "sdk/packages/rust/iii/Cargo.toml", 'version = "0.15.0-next.1"\n')
    _write(root / "sdk/packages/rust/observability/Cargo.toml", 'version = "0.13.0-next.1"\n')
    _write(root / "sdk/packages/rust/helpers/Cargo.toml", 'version = "0.13.0-next.1"\n')
    _write(root / "sdk/packages/node/iii/package.json", '{\n  "version": "0.15.0-next.1"\n}\n')
    _write(root / "sdk/packages/node/iii-browser/package.json", '{\n  "version": "0.15.0-next.1"\n}\n')
    _write(root / "sdk/packages/node/observability/package.json", '{\n  "version": "0.13.0-next.1"\n}\n')
    _write(root / "sdk/packages/node/helpers/package.json", '{\n  "version": "0.13.0-next.1"\n}\n')
    _write(root / "sdk/packages/python/iii/pyproject.toml", (
        'version = "0.15.0.dev1"\n'
        'dependencies = [\n    "iii-observability==0.13.0.dev1",\n]\n'
    ))
    _write(root / "sdk/packages/python/observability/pyproject.toml", 'version = "0.13.0.dev1"\n')
    _write(root / "sdk/packages/python/helpers/pyproject.toml", 'version = "0.13.0.dev1"\n')
    _write(root / "console/packages/console-rust/Cargo.toml", 'version = "0.15.0-next.1"\n')

    script = Path(__file__).parent / "bump_manifests.py"
    result = subprocess.run(
        [sys.executable, str(script), "--root", str(root),
         "--version", "0.16.0-next.2", "--python-version", "0.16.0.dev2"],
        check=True, capture_output=True, text=True,
    )
    assert "0.16.0-next.2" in result.stdout

    assert 'version = "0.16.0-next.2"' in (root / "Cargo.toml").read_text()
    assert 'iii-observability==0.16.0.dev2' in (root / "sdk/packages/python/iii/pyproject.toml").read_text()
