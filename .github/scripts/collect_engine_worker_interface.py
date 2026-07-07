#!/usr/bin/env python3
import argparse
import json
import pathlib
import subprocess
import sys
import time

from build_engine_publish_payload import normalize_worker_interface


def count_worker_matches(workers_json: dict[str, object], worker_name: str) -> int:
    workers = workers_json.get("workers", [])
    if not isinstance(workers, list):
        return 0
    return sum(
        1
        for worker in workers
        if isinstance(worker, dict)
        and (worker.get("name") == worker_name or worker.get("id") == worker_name)
    )


def run_iii(function_path: str, payload: dict[str, object]) -> dict[str, object]:
    completed = subprocess.run(
        [
            "iii",
            "trigger",
            function_path,
            "--json",
            json.dumps(payload),
        ],
        check=True,
        text=True,
        capture_output=True,
        timeout=60,
    )
    return json.loads(completed.stdout)


def wait_for_worker(worker_name: str, wait_seconds: int) -> dict[str, object]:
    # `engine::workers::list` is lean post-rework (no per-worker `functions[]`),
    # but we only need it to confirm the worker is reachable. The function
    # surface is fetched via `engine::workers::info` below.
    deadline = time.monotonic() + wait_seconds
    workers_json = run_iii("engine::workers::list", {})
    while count_worker_matches(workers_json, worker_name) != 1 and time.monotonic() < deadline:
        time.sleep(2)
        workers_json = run_iii("engine::workers::list", {})
    return workers_json


def collect_worker_info(worker_name: str) -> dict[str, object]:
    # The detailed info envelope is the source of truth for a worker's surface
    # in the new engine_fn world. The `worker` field mirrors a workers::list
    # row; `functions`, `trigger_types`, and `registered_triggers` arrays are
    # appended.
    return run_iii("engine::workers::info", {"name": worker_name})


# `engine::functions::info` accepts at most 32 ids per `function_ids` batch.
FUNCTIONS_INFO_BATCH_MAX = 32


def collect_function_details(function_ids: list[str]) -> list[dict[str, object]]:
    # `engine::functions::list` is lean post-rework — schemas live behind
    # `engine::functions::info`. Its `function_ids` batch mode fetches up
    # to 32 details per round-trip; a per-item `error` marker ("forbidden"
    # / "not_found") fails the collection, matching the old per-id loop.
    details: list[dict[str, object]] = []
    for start in range(0, len(function_ids), FUNCTIONS_INFO_BATCH_MAX):
        chunk = function_ids[start : start + FUNCTIONS_INFO_BATCH_MAX]
        try:
            response = run_iii("engine::functions::info", {"function_ids": chunk})
        except (subprocess.CalledProcessError, subprocess.TimeoutExpired, json.JSONDecodeError) as exc:
            raise RuntimeError(
                f"could not collect function info for batch {chunk!r}: {exc}"
            ) from exc
        entries = response.get("functions")
        if not isinstance(entries, list):
            raise RuntimeError(
                f"functions::info batch response missing `functions` array: {response!r}"
            )
        for entry in entries:
            if not isinstance(entry, dict):
                raise RuntimeError(
                    f"functions::info batch entry is not an object: {entry!r}"
                )
            if entry.get("error"):
                raise RuntimeError(
                    f"could not collect function info for "
                    f"{entry.get('function_id')!r}: {entry['error']}"
                )
            details.append(entry)
    return details


def collect_trigger_type_details(trigger_type_ids: list[str]) -> list[dict[str, object]]:
    # Same shape concern as functions: `engine::triggers::list` is lean, so
    # we fetch per-type schemas via `engine::triggers::info`. Trigger types
    # are the primary content for infrastructure workers (iii-http, iii-cron,
    # iii-bridge, ...) — failing closed if any lookup errors out is
    # intentional to avoid masking the real surface of the worker.
    details: list[dict[str, object]] = []
    for tt_id in trigger_type_ids:
        try:
            details.append(run_iii("engine::triggers::info", {"id": tt_id}))
        except (subprocess.CalledProcessError, subprocess.TimeoutExpired, json.JSONDecodeError) as exc:
            raise RuntimeError(
                f"could not collect trigger type info for {tt_id!r}: {exc}"
            ) from exc
    return details


def collect_baseline_trigger_type_ids(baseline_path: pathlib.Path) -> list[str]:
    # The baseline snapshot is captured BEFORE the target worker is reloaded
    # into the engine, so it contains the always-on (`mandatory`) trigger
    # types. We use the lean `triggers::list` shape here — only ids are
    # needed for diffing.
    if not baseline_path.exists():
        return []
    raw = json.loads(baseline_path.read_text(encoding="utf-8"))
    triggers = raw.get("triggers") or raw.get("trigger_types") or []
    return [t.get("id") for t in triggers if isinstance(t, dict) and isinstance(t.get("id"), str)]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--worker", required=True)
    parser.add_argument("--out", default="worker-interface.json")
    parser.add_argument("--wait-seconds", type=int, default=0)
    parser.add_argument("--trigger-types-baseline", default="")
    args = parser.parse_args()

    baseline_ids: list[str] = []
    if args.trigger_types_baseline:
        baseline_ids = collect_baseline_trigger_type_ids(
            pathlib.Path(args.trigger_types_baseline)
        )

    wait_for_worker(args.worker, args.wait_seconds)
    worker_info = collect_worker_info(args.worker)

    functions = worker_info.get("functions") or []
    if not isinstance(functions, list):
        raise ValueError("workers::info `functions` must be an array")
    function_ids = [
        f.get("function_id")
        for f in functions
        if isinstance(f, dict) and isinstance(f.get("function_id"), str)
    ]

    trigger_types = worker_info.get("trigger_types") or []
    if not isinstance(trigger_types, list):
        raise ValueError("workers::info `trigger_types` must be an array")
    trigger_type_ids = [
        t.get("id")
        for t in trigger_types
        if isinstance(t, dict) and isinstance(t.get("id"), str)
    ]

    function_details = collect_function_details(function_ids)
    trigger_type_details = collect_trigger_type_details(trigger_type_ids)

    interface = normalize_worker_interface(
        worker_name=args.worker,
        worker_info=worker_info,
        function_details=function_details,
        trigger_type_details=trigger_type_details,
        baseline_trigger_type_ids=baseline_ids,
    )
    pathlib.Path(args.out).write_text(json.dumps(interface, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(interface, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
