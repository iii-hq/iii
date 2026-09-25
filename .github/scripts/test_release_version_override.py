"""Validate exact release selection and the real workflow calculation step.

No release is dispatched: CLI integration runs against temporary local Git repos.
"""
from __future__ import annotations

import os
from pathlib import Path
import shutil
import subprocess

import pytest
import yaml

import calculate_release_version as versions


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW_PATH = ROOT / ".github/workflows/create-tag.yml"
TAGS = ["iii/v0.24.0", "iii/v0.24.0-rc.2", "iii/v0.25.0-rc.1"]


def select(override, *, bump="patch", channel="none", tags=None, stable="0.24.0"):
    """Exercise the same selection function used by both release workflows."""
    return versions.calculate_version(
        "0.25.0-rc.1", bump, channel, stable, TAGS if tags is None else tags,
        "iii", version_override=override,
    )


@pytest.mark.parametrize("bump", ["patch", "minor", "major", "none"])
def test_explicit_patch_overrides_prerelease_promotion(bump):
    """Recovery can choose 0.24.1 without deleting the published 0.25 prerelease."""
    assert select("0.24.1", bump=bump) == "0.24.1"


@pytest.mark.parametrize("override", [None, ""])
def test_omitted_or_empty_override_preserves_existing_promotion(override):
    assert select(override) == "0.25.0"


def test_empty_override_preserves_none_bump_validation():
    with pytest.raises(ValueError, match="requires a prerelease"):
        select("", bump="none")


@pytest.mark.parametrize("channel", ["alpha", "beta", "rc", "next"])
def test_exact_prerelease_keeps_the_requested_counter(channel):
    assert select(f"0.24.1-{channel}.7", channel=channel) == f"0.24.1-{channel}.7"


@pytest.mark.parametrize("override", [
    " ", " 0.24.1", "0.24.1 ", "0.24.1\n", "0.24.1\r\n",
    "v0.24.1", "iii/v0.24.1", "0.24", "0.24.1.2", "-0.24.1",
    "00.24.1", "0.024.1", "0.24.01", "0.24.1-rc.01", "0.24.1-rc.-1",
    "0.24.1-RC.1", "0.24.1-dev.1", "0.24.1-rc", "0.24.1+build.1",
    "0.24.1-dry-run.1", "０.24.1", "0.24.1; echo injected", "$(touch injected)",
])
def test_rejects_noncanonical_or_unsafe_override(override):
    with pytest.raises(ValueError, match="exact version"):
        select(override)


@pytest.mark.parametrize("override,channel", [
    ("0.24.1", "rc"), ("0.24.1-rc.1", "none"),
    ("0.24.1-beta.1", "rc"), ("0.24.1-rc.1", "next"),
])
def test_rejects_version_channel_mismatch(override, channel):
    with pytest.raises(ValueError, match="match the selected prerelease"):
        select(override, channel=channel)


@pytest.mark.parametrize("override,channel", [
    ("0.24.0", "none"), ("0.23.99", "none"),
    ("0.24.0-rc.9", "rc"), ("0.23.99-rc.9", "rc"),
])
def test_rejects_a_base_at_or_below_latest_stable(override, channel):
    with pytest.raises(ValueError, match="newer than the latest stable"):
        select(override, channel=channel)


def test_stable_comparison_is_numeric_and_target_scoped():
    assert select("0.24.10", stable="0.24.9") == "0.24.10"
    assert select("0.24.1", tags=[*TAGS, "console/v0.24.1"]) == "0.24.1"
    assert select("0.1.0", stable=None, tags=[]) == "0.1.0"


@pytest.mark.parametrize("version,channel", [("0.24.1", "none"), ("0.24.1-rc.4", "rc")])
def test_rejects_an_existing_exact_tag(version, channel):
    with pytest.raises(ValueError, match="already exists"):
        select(version, channel=channel, tags=[*TAGS, f"iii/v{version}"])


def test_override_does_not_bypass_argument_validation():
    with pytest.raises(ValueError, match="unknown bump_type"):
        select("0.24.1", bump="bogus")
    with pytest.raises(ValueError, match="unknown prerelease"):
        select("0.24.1", channel="bogus")


@pytest.fixture
def workflow():
    """BaseLoader preserves the Actions 'on' key rather than interpreting YAML 1.1 booleans."""
    return yaml.load(WORKFLOW_PATH.read_text(), Loader=yaml.BaseLoader)


def step(workflow, name):
    return next(item for item in workflow["jobs"]["create-tag"]["steps"] if item["name"] == name)


def test_workflow_uses_optional_input_and_environment_not_shell_interpolation(workflow):
    override = workflow["on"]["workflow_dispatch"]["inputs"]["version_override"]
    assert override["type"] == "string"
    assert override["required"] == "false"
    assert override["default"] == ""
    calc = step(workflow, "Calculate versions")
    assert calc["env"]["VERSION_OVERRIDE"] == "${{ inputs.version_override }}"
    assert '--version-override "$VERSION_OVERRIDE"' in calc["run"]
    for item in workflow["jobs"]["create-tag"]["steps"]:
        assert "${{ inputs.version_override }}" not in item.get("run", "")
    preflight = step(workflow, "Pre-flight checks")
    assert '-z "$VERSION_OVERRIDE"' in preflight["run"]
    assert '"$BRANCH" != "main"' in preflight["run"]
    names = [s["name"] for s in workflow["jobs"]["create-tag"]["steps"]]
    assert names.index("Calculate versions") < names.index("Update iii manifests")
    assert '--version "${NEW_VERSION}"' in step(workflow, "Update iii manifests")["run"]
    assert 'run: cargo update' not in step(workflow, "Calculate versions")["run"]
    assert step(workflow, "Sync Cargo.lock")["run"] == "cargo update --workspace"


def test_changes_to_create_tag_trigger_script_ci():
    doc = yaml.load((ROOT / ".github/workflows/test-scripts.yml").read_text(), Loader=yaml.BaseLoader)
    for event in ["push", "pull_request"]:
        assert ".github/workflows/create-tag.yml" in doc["on"][event]["paths"]


@pytest.fixture
def release_repo(tmp_path):
    """Use only local tags/manifests, with no remote or publication credentials."""
    repo = tmp_path / "repo"
    (repo / ".github/scripts").mkdir(parents=True)
    shutil.copyfile(ROOT / ".github/scripts/calculate_release_version.py", repo / ".github/scripts/calculate_release_version.py")
    (repo / "engine").mkdir()
    (repo / "engine/Cargo.toml").write_text('[package]\nname = "iii"\nversion = "0.25.0-rc.1"\n')
    for args in [
        ["init", "-q"], ["config", "user.name", "Release Test"],
        ["config", "user.email", "release-test@example.invalid"],
        ["add", "."], ["-c", "commit.gpgsign=false", "commit", "-qm", "fixture"],
    ]:
        subprocess.run(["git", *args], cwd=repo, check=True, capture_output=True)
    for tag in TAGS:
        subprocess.run(["git", "tag", tag], cwd=repo, check=True, capture_output=True)
    return repo


def calculate_step(workflow, repo, output, override="0.24.1", channel="none", dry_run=False):
    """Execute the workflow's real Bash calculation step, never any publishing step."""
    env = dict(os.environ, TARGET="iii", BUMP="patch", PRERELEASE=channel,
               VERSION_OVERRIDE=override, DRY_RUN="true" if dry_run else "false",
               GITHUB_OUTPUT=str(output))
    return subprocess.run(["bash", "-euo", "pipefail", "-c", step(workflow, "Calculate versions")["run"]],
                          cwd=repo, env=env, text=True, capture_output=True, timeout=15)


@pytest.mark.parametrize("override,channel,dry_run,expected,python_version,npm_tag", [
    ("0.24.1", "none", False, "0.24.1", "0.24.1", "latest"),
    ("0.24.1-rc.7", "rc", False, "0.24.1-rc.7", "0.24.1rc7", "rc"),
    ("", "none", False, "0.25.0", "0.25.0", "latest"),
    ("0.24.1", "none", True, "0.24.1-dry-run.1", "0.24.1-dry-run.1", "latest"),
])
def test_workflow_calculation_outputs_and_no_mutation(
    workflow, release_repo, tmp_path, override, channel, dry_run, expected, python_version, npm_tag
):
    output = tmp_path / "outputs.txt"
    before = subprocess.check_output(["git", "tag", "-l"], cwd=release_repo)
    result = calculate_step(workflow, release_repo, output, override, channel, dry_run)
    assert result.returncode == 0, result.stderr
    values = dict(line.split("=", 1) for line in output.read_text().splitlines())
    assert values["version"] == expected
    assert values["tag"] == f"iii/v{expected}"
    assert values["python_version"] == python_version
    assert values["npm_tag"] == npm_tag
    assert values["is_prerelease"] == ("false" if channel == "none" else "true")
    assert values["current"] == "0.25.0-rc.1"
    assert subprocess.check_output(["git", "tag", "-l"], cwd=release_repo) == before
    assert not subprocess.check_output(["git", "status", "--porcelain"], cwd=release_repo)


@pytest.mark.parametrize("override", ["$(touch injected)", "0.24.1\ntag=evil", "0.24.0", "0.24.1-rc.1"])
def test_invalid_input_emits_no_outputs_and_cannot_execute_shell(workflow, release_repo, tmp_path, override):
    output = tmp_path / "outputs.txt"
    result = calculate_step(workflow, release_repo, output, override)
    assert result.returncode != 0
    assert not output.exists() or output.read_text() == ""
    assert not (release_repo / "injected").exists()
    assert not subprocess.check_output(["git", "status", "--porcelain"], cwd=release_repo)


def test_existing_tag_fails_before_outputs_even_in_dry_run(workflow, release_repo, tmp_path):
    subprocess.run(["git", "tag", "iii/v0.24.1-rc.7"], cwd=release_repo, check=True)
    output = tmp_path / "outputs.txt"
    result = calculate_step(
        workflow, release_repo, output, override="0.24.1-rc.7", channel="rc", dry_run=True
    )
    assert result.returncode != 0
    assert "already exists" in result.stderr
    assert not output.exists() or not output.read_text()


def test_summary_reports_validated_selection(workflow, tmp_path):
    env = dict(os.environ, CURRENT_VERSION="0.25.0-rc.1", SELECTED_VERSION="0.24.1",
               SELECTED_TAG="iii/v0.24.1", VERSION_OVERRIDE="0.24.1",
               GITHUB_STEP_SUMMARY=str(tmp_path / "summary.md"))
    result = subprocess.run(["bash", "-euo", "pipefail", "-c", step(workflow, "Summarize selected release")["run"]],
                            env=env, capture_output=True, text=True, timeout=5)
    assert result.returncode == 0, result.stderr
    summary = (tmp_path / "summary.md").read_text()
    assert "`0.24.1`" in summary and "`iii/v0.24.1`" in summary
    assert "bump was ignored" in summary and "not a historical maintenance branch" in summary
