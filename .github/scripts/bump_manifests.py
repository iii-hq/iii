#!/usr/bin/env python3
"""Rewrite every release-bumped manifest in lockstep.

Pure functions live at module top so they can be unit-tested. The CLI at
the bottom drives the rewrites the create-tag workflow used to perform
inline with sed/jq.
"""
from __future__ import annotations

import re


_CARGO_PACKAGE_VERSION_RE = re.compile(r'^version = "[^"]*"', re.MULTILINE)


def bump_cargo_package_version(text: str, new_version: str) -> str:
    """Replace the first top-level ``version = "..."`` line in a Cargo manifest."""
    m = _CARGO_PACKAGE_VERSION_RE.search(text)
    if m is None:
        raise ValueError("no top-level version line in Cargo manifest")
    start, end = m.span()
    return f'{text[:start]}version = "{new_version}"{text[end:]}'


def bump_cargo_workspace_dep_version(text: str, dep_name: str, new_version: str) -> str:
    """Replace the ``version = "..."`` value inside a ``dep_name = { ... }`` line.

    Matches a single workspace-dependency entry whose key is exactly
    ``dep_name`` and rewrites the embedded ``version`` field. Other fields
    on the line (``path``, ``features``, ...) are preserved.
    """
    line_re = re.compile(
        rf'^(?P<lead>{re.escape(dep_name)}\s*=\s*\{{[^}}\n]*?version\s*=\s*")[^"]*(?P<tail>"[^}}\n]*\}})',
        re.MULTILINE,
    )
    m = line_re.search(text)
    if m is None:
        raise ValueError(f"no workspace dependency entry for {dep_name!r}")
    return f'{text[:m.start()]}{m.group("lead")}{new_version}{m.group("tail")}{text[m.end():]}'


_JSON_TOP_VERSION_RE = re.compile(r'^(\s*)"version"(\s*):(\s*)"[^"]*"', re.MULTILINE)


def bump_json_top_level_version(text: str, new_version: str) -> str:
    """Replace the first top-level ``"version": "..."`` line in a JSON file.

    Operates textually to preserve formatting. The npm/iii workflow uses
    ``jq`` today; we replace it with this so the workflow has a single
    invocation point and the behaviour is unit-tested.
    """
    m = _JSON_TOP_VERSION_RE.search(text)
    if m is None:
        raise ValueError("no top-level version key in JSON manifest")
    indent, sp1, sp2 = m.group(1), m.group(2), m.group(3)
    replacement = f'{indent}"version"{sp1}:{sp2}"{new_version}"'
    return f'{text[:m.start()]}{replacement}{text[m.end():]}'


def bump_pep440_dep_pin(text: str, dep_name: str, new_pep440: str) -> str:
    """Replace ``"<dep_name>==<old>"`` with ``"<dep_name>==<new_pep440>"``.

    Replaces a pinned PEP 440 dependency version in a pyproject.toml
    ``dependencies = [...]`` array.
    """
    line_re = re.compile(rf'"{re.escape(dep_name)}==[^"]*"')
    m = line_re.search(text)
    if m is None:
        raise ValueError(f"no pinned dependency entry for {dep_name!r}")
    return f'{text[:m.start()]}"{dep_name}=={new_pep440}"{text[m.end():]}'


_GO_SDK_VERSION_RE = re.compile(r'const sdkVersion = "[^"]*"')


def bump_go_const_version(text: str, new_version: str) -> str:
    """Replace the ``const sdkVersion = "..."`` line in the Go SDK client.

    Go modules are versioned by their git tag, but the const is reported in
    worker metadata, so it must stay in lockstep with the other SDKs. Uses
    the raw semver string (not the PEP 440 form).
    """
    m = _GO_SDK_VERSION_RE.search(text)
    if m is None:
        raise ValueError("no sdkVersion const in Go client")
    return f'{text[:m.start()]}const sdkVersion = "{new_version}"{text[m.end():]}'


from pathlib import Path


_CARGO_PACKAGE_FILES = (
    "engine/Cargo.toml",
    "sdk/packages/rust/iii/Cargo.toml",
    "sdk/packages/rust/observability/Cargo.toml",
    "sdk/packages/rust/helpers/Cargo.toml",
    "console/packages/console-rust/Cargo.toml",
)
_JSON_PACKAGE_FILES = (
    "sdk/packages/node/iii/package.json",
    "sdk/packages/node/iii-browser/package.json",
    "sdk/packages/node/observability/package.json",
    "sdk/packages/node/helpers/package.json",
)


def rewrite_all(root: Path, new_version: str, new_py_version: str) -> None:
    """Rewrite every release manifest under ``root`` in lockstep."""
    # Cargo package versions
    for rel in _CARGO_PACKAGE_FILES:
        path = root / rel
        path.write_text(bump_cargo_package_version(path.read_text(), new_version))

    # Workspace root: bump the workspace.package version and the internal
    # workspace-dep version pins. The pins are plain "X.Y.Z" requirements, so
    # they must move to the prerelease too — otherwise `cargo` can't match the
    # prerelease candidate (e.g. ^0.19.4 won't accept 0.19.4-alpha.1).
    workspace_path = root / "Cargo.toml"
    body = bump_cargo_package_version(workspace_path.read_text(), new_version)
    body = bump_cargo_workspace_dep_version(body, "iii-observability", new_version)
    body = bump_cargo_workspace_dep_version(body, "iii-helpers", new_version)
    workspace_path.write_text(body)

    # JSON package versions
    for rel in _JSON_PACKAGE_FILES:
        path = root / rel
        path.write_text(bump_json_top_level_version(path.read_text(), new_version))

    # Python iii: top-level version + internal pins (== requirements must
    # move to the prerelease alongside the packages they point at).
    # Note: iii-observability is no longer a direct dep of Python iii (replaced
    # by iii-helpers after the observability-into-helpers refactor).
    py_iii = root / "sdk/packages/python/iii/pyproject.toml"
    body = bump_cargo_package_version(py_iii.read_text(), new_py_version)
    body = bump_pep440_dep_pin(body, "iii-helpers", new_py_version)
    py_iii.write_text(body)

    # Python observability: top-level version + iii-helpers dep pin (shim must
    # depend on the same version of iii-helpers being released alongside it).
    py_obs = root / "sdk/packages/python/observability/pyproject.toml"
    body = bump_cargo_package_version(py_obs.read_text(), new_py_version)
    body = bump_pep440_dep_pin(body, "iii-helpers", new_py_version)
    py_obs.write_text(body)

    # Go SDK: keep the reported sdkVersion const in lockstep. The module
    # itself is versioned by its git tag; this only updates the metadata
    # const, using the raw semver (not the PEP 440) version.
    go_client = root / "sdk/packages/go/iii/client.go"
    go_client.write_text(bump_go_const_version(go_client.read_text(), new_version))

    # Python helpers: top-level version only
    py_helpers = root / "sdk/packages/python/helpers/pyproject.toml"
    py_helpers.write_text(bump_cargo_package_version(py_helpers.read_text(), new_py_version))


import argparse
import sys


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", required=True, type=Path)
    parser.add_argument("--version", required=True)
    parser.add_argument("--python-version", required=True)
    args = parser.parse_args(argv)

    rewrite_all(root=args.root, new_version=args.version, new_py_version=args.python_version)
    print(f"Bumped manifests to {args.version} (python: {args.python_version})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
