"""The Load Benchmarks workflow runs every iii-loadtest scenario.

`iii-loadtest suite` refuses a scenario name it does not know before its first
step, so a stale list in the workflow fails every tag, scheduled and default
dispatch run.

Run with: python -m pytest .github/scripts/test_loadtest_workflow.py -v
"""

from __future__ import annotations

import re
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github/workflows/loadtest.yml"
SCENARIOS = ROOT / "crates/iii-loadtest/src/scenarios/mod.rs"
SUITE = ROOT / "crates/iii-loadtest/src/suite.rs"


def known_scenarios() -> str:
    """`scenarios::ALL`, comma-joined in its order."""
    source = SCENARIOS.read_text(encoding="utf-8")
    body = re.search(r"pub const ALL: &\[&str\] = &\[(.*?)\];", source, re.S)
    assert body, "scenarios::ALL not found"
    names = re.findall(r'"([^"]+)"', body.group(1))
    assert names, "scenarios::ALL is empty"
    return ",".join(names)


def workflow() -> dict:
    # PyYAML reads the bare `on:` key as the boolean True (YAML 1.1).
    return yaml.safe_load(WORKFLOW.read_text(encoding="utf-8"))


def test_dispatch_default_is_every_scenario() -> None:
    triggers = workflow()[True]
    default = triggers["workflow_dispatch"]["inputs"]["scenarios"]["default"]
    assert default == known_scenarios()


def test_tag_and_scheduled_runs_use_every_scenario() -> None:
    # `${{ inputs.scenarios || '<list>' }}`: tag and scheduled runs have no
    # inputs, so they run the quoted list.
    expression = workflow()["jobs"]["loadtest"]["env"]["SCENARIOS"]
    fallback = re.search(r"\|\|\s*'([^']*)'", expression)
    assert fallback, f"no fallback list in {expression!r}"
    assert fallback.group(1) == known_scenarios()


def test_suite_default_is_every_scenario() -> None:
    source = SUITE.read_text(encoding="utf-8")
    default = re.search(
        r'default_value = "([^"]*)"\s*\)\]\s*pub scenarios: Vec<String>', source
    )
    assert default, "--scenarios default not found in suite.rs"
    assert default.group(1) == known_scenarios()
