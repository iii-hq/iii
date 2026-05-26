"""Unit tests for bump_manifests.py.

Run with: python -m pytest .github/scripts/test_bump_manifests.py -v
"""
from __future__ import annotations

import textwrap

import pytest

from bump_manifests import bump_cargo_package_version
from bump_manifests import bump_cargo_workspace_dep_version
from bump_manifests import bump_json_top_level_version
from bump_manifests import bump_pep440_dep_pin


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
