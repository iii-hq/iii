"""Regression tests for the workflow that publishes iii on the registry.

Run with: python -m pytest .github/scripts/test_publish_engine_registry_workflow.py -v
"""
from __future__ import annotations

import hashlib
import os
import subprocess
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github/workflows/_publish-engine-registry.yml"


def load(path: Path) -> dict:
    return yaml.safe_load(path.read_text(encoding="utf-8"))


def steps() -> dict[str, str]:
    return {step.get("name", ""): step.get("run", "") for step in load(WORKFLOW)["jobs"]["publish"]["steps"]}


def test_it_publishes_the_engine_installer_with_the_release_archives() -> None:
    publish = steps()["Publish"]
    assert "registry publish" in publish
    assert "--install-script install.sh" in publish
    assert '--artifacts "$GITHUB_WORKSPACE/dist"' in publish
    assert '--tag "$REGISTRY_TAG"' in publish
    job = load(WORKFLOW)["jobs"]["publish"]
    working_directory = next(s for s in job["steps"] if s.get("name") == "Publish")["working-directory"]
    manifest = load(ROOT / working_directory / "iii.worker.yaml")
    assert (manifest["name"], manifest["deploy"], manifest["bin"]) == ("iii", "executable", "iii")
    assert (ROOT / working_directory / "install.sh").is_file()
    assert (ROOT / working_directory / "README.md").is_file()


GH_STUB = """#!/bin/sh
# gh release download TAG --repo REPO --dir DIR --pattern ...: the fixture release.
[ "$1 $2" = "release download" ] || exit 2
while [ $# -gt 0 ]; do
  [ "$1" = --dir ] && dir="$2"
  shift
done
cp "$FIXTURE_RELEASE"/* "$dir"/
"""


def release_asset(release: Path, name: str, content: bytes, digest: bytes | None = None) -> None:
    """An archive and, unless digest is b"", the .sha256 beside it in the release's format."""
    (release / name).write_bytes(content)
    if digest == b"":
        return
    stem = name.removesuffix(".tar.gz").removesuffix(".zip")
    sha = hashlib.sha256(digest if digest is not None else content).hexdigest()
    (release / f"{stem}.sha256").write_text(f"{sha}  {name}\n")


def download(tmp_path: Path, assets: list[tuple[str, bytes, bytes | None]]) -> tuple[subprocess.CompletedProcess, Path]:
    """Runs the workflow's download step against a fixture release."""
    release, stub, work, runner_temp = (tmp_path / d for d in ("fixture", "bin", "work", "runner"))
    for d in (release, stub, work, runner_temp):
        d.mkdir()
    for name, content, digest in assets:
        release_asset(release, name, content, digest)
    (stub / "gh").write_text(GH_STUB)
    (stub / "gh").chmod(0o755)
    env = {
        **os.environ,
        "PATH": f"{stub}{os.pathsep}{os.environ['PATH']}",
        "FIXTURE_RELEASE": str(release),
        "RUNNER_TEMP": str(runner_temp),
        "RELEASE_TAG": "iii/v0.27.0",
        "GITHUB_REPOSITORY": "iii-hq/iii",
    }
    script = steps()["Download the release archives"]
    # How Actions runs a `run:` step with `shell: bash`.
    result = subprocess.run(
        ["bash", "--noprofile", "--norc", "-eo", "pipefail", "-c", script],
        cwd=work, env=env, capture_output=True, text=True,
    )
    return result, work / "dist"


def test_only_the_engine_archives_reach_dist(tmp_path: Path) -> None:
    result, dist = download(
        tmp_path,
        [
            ("iii-x86_64-unknown-linux-musl.tar.gz", b"musl", None),
            ("iii-armv7-unknown-linux-gnueabihf.tar.gz", b"armv7", None),
            ("iii-x86_64-pc-windows-msvc.zip", b"windows", None),
            # 0.25 and older also ship these: never the engine's archives.
            ("iii-worker-x86_64-unknown-linux-gnu.tar.gz", b"worker", None),
            ("iii-init-x86_64-unknown-linux-musl.tar.gz", b"init", None),
        ],
    )
    assert result.returncode == 0, result.stdout + result.stderr
    assert sorted(p.name for p in dist.iterdir()) == [
        "iii-armv7-unknown-linux-gnueabihf.tar.gz",
        "iii-x86_64-pc-windows-msvc.zip",
        "iii-x86_64-unknown-linux-musl.tar.gz",
    ]


def test_an_archive_that_does_not_match_its_checksum_stops_the_publish(tmp_path: Path) -> None:
    result, _ = download(
        tmp_path,
        [
            ("iii-x86_64-unknown-linux-musl.tar.gz", b"musl", None),
            ("iii-aarch64-apple-darwin.tar.gz", b"tampered", b"what was built"),
        ],
    )
    assert result.returncode != 0
    assert "::error::iii-aarch64-apple-darwin.tar.gz does not match iii-aarch64-apple-darwin.sha256" in result.stdout


def test_an_archive_without_a_checksum_stops_the_publish(tmp_path: Path) -> None:
    result, _ = download(tmp_path, [("iii-x86_64-unknown-linux-musl.tar.gz", b"musl", b"")])
    assert result.returncode != 0
    assert "::error::iii/v0.27.0 has iii-x86_64-unknown-linux-musl.tar.gz but no iii-x86_64-unknown-linux-musl.sha256" in result.stdout


def test_a_release_without_engine_archives_stops_the_publish(tmp_path: Path) -> None:
    result, _ = download(tmp_path, [("iii-worker-x86_64-unknown-linux-gnu.tar.gz", b"worker", None)])
    assert result.returncode != 0
    assert "::error::iii/v0.27.0 has no iii-<target> archives" in result.stdout


def test_a_prerelease_never_goes_under_latest() -> None:
    meta = steps()["Read the version"]
    assert 'REGISTRY_TAG" == latest && "$version" == *-*' in meta


def test_the_published_version_is_installed_back_with_its_own_installer() -> None:
    check = steps()["Install it back from the registry"]
    assert "x-executable-installer" in check
    assert "custom" in check
    assert '--version | awk' in check


def test_both_release_pipelines_publish_iii_before_its_skills() -> None:
    for name, tag in (("release-iii.yml", "${{ needs.setup.outputs.npm_tag }}"), ("alpha-release.yml", "alpha")):
        jobs = load(ROOT / ".github/workflows" / name)["jobs"]
        job = jobs["publish-engine-registry"]
        assert job["uses"] == "./.github/workflows/_publish-engine-registry.yml"
        assert job["with"]["registry_tag"] == tag
        assert "engine-release" in job["needs"]
        assert "publish-engine-registry" in jobs["publish-worker-skills"]["needs"]
