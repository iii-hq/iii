"""Regression tests for the workflow that publishes iii on the registry.

Run with: python -m pytest .github/scripts/test_publish_engine_registry_workflow.py -v
"""
from __future__ import annotations

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


def test_only_checked_engine_archives_are_uploaded() -> None:
    download = steps()["Download the release archives"]
    assert "iii-worker" in download  # named as what is left out
    assert "sha256sum" in download
    assert ".sha256" in download


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
