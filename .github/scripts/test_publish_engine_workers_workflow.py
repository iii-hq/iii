"""Regression tests for the reusable engine-worker publish workflow.

Run with: python -m pytest .github/scripts/test_publish_engine_workers_workflow.py -v
"""
from __future__ import annotations

from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github/workflows/_publish-engine-workers.yml"


def steps() -> dict[str, str]:
    workflow = yaml.safe_load(WORKFLOW.read_text(encoding="utf-8"))
    return {
        step.get("name", ""): step.get("run", "")
        for step in workflow["jobs"]["publish"]["steps"]
    }


def test_the_engine_is_called_with_iii_call() -> None:
    runs = "\n".join(steps().values())
    assert "iii call 'engine::workers::list'" in runs
    assert "iii trigger" not in runs


def test_no_step_reads_trigger_types() -> None:
    text = WORKFLOW.read_text(encoding="utf-8")
    assert "engine::triggers::" not in text
    assert "trigger-types-baseline" not in text
    collect = steps()["Collect worker interface"]
    assert "collect_engine_worker_interface.py" in collect


def test_the_reload_leaves_out_a_worker_the_engine_already_runs() -> None:
    # config.yaml refuses the mandatory workers (UNSUPPORTED_CONFIG_WORKERS),
    # and a refused reload stops the engine.
    reload = next(run for name, run in steps().items() if name.startswith("Reload engine"))
    assert "engine-workers.json" in reload
    assert "already runs in the engine" in reload
