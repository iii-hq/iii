"""Unit tests for bump_manifests.py.

Run with: python -m pytest .github/scripts/test_bump_manifests.py -v
"""
from __future__ import annotations

import textwrap

import pytest

from bump_manifests import bump_cargo_package_version


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
