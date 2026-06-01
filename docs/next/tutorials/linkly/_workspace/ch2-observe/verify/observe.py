#!/usr/bin/env python3
"""Ch2 observability checks against a running iii engine.

  Issue 1 - RESOLVED in 0.16.x: 3xx redirects now tag span status "ok"
            (was "error"). issue1_span_status() stays as a regression check.
            Engine semantics: 1xx/2xx/3xx -> ok, 4xx/5xx -> error.
  Issue 2 - engine::traces::list sort_by=duration_ms is inverted / unsorted
  Works   - structured worker logs; the cross-worker trace waterfall

Usage (engine running, after generate_traffic.sh):
  python3 observe.py
"""
import json
import subprocess
import sys


def trig(fn: str, payload: dict) -> dict:
    """Invoke an engine function via `iii trigger` and parse the JSON result."""
    proc = subprocess.run(
        ["iii", "trigger", fn, "--json", json.dumps(payload)],
        capture_output=True,
        text=True,
    )
    out = proc.stdout
    start, end = out.find("{"), out.rfind("}")
    if start == -1 or end == -1:
        sys.exit(f"!! {fn} returned no JSON:\nstdout={out}\nstderr={proc.stderr}")
    return json.loads(out[start : end + 1])


def attr_map(span: dict) -> dict:
    """Span attributes come back as [key, value] pairs; fold to a dict safely."""
    m = {}
    for item in span.get("attributes", []):
        if isinstance(item, (list, tuple)) and len(item) == 2:
            m[item[0]] = item[1]
    return m


def dur_ms(span: dict) -> float:
    return (span["end_time_unix_nano"] - span["start_time_unix_nano"]) / 1e6


def issue1_span_status() -> None:
    print("== Issue 1: span status by HTTP code ==")
    data = trig("engine::traces::list", {"limit": 30})
    seen = set()
    for span in data["spans"]:
        code = attr_map(span).get("http.response.status_code")
        if code is None:
            continue
        line = "{:14} http={}  span_status={}".format(span["name"], code, span["status"])
        if line not in seen:
            seen.add(line)
            print("  " + line)
    print()


def issue2_duration_sort() -> None:
    print("== Issue 2: engine::traces::list sort_by=duration_ms ==")
    for order in ("desc", "asc"):
        data = trig(
            "engine::traces::list",
            {"name": "GET /s/:code", "sort_by": "duration_ms", "sort_order": order, "limit": 6},
        )
        vals = [round(dur_ms(s), 3) for s in data["spans"]]
        want = "SLOWEST first" if order == "desc" else "FASTEST first"
        print("  {:4} (want {}): {}".format(order, want, vals))
    print()


def works_logs() -> None:
    print("== Works: worker logs (engine::logs::list) ==")
    data = trig("engine::logs::list", {"limit": 100})
    n = 0
    for entry in data["logs"]:
        if entry.get("body") in ("link resolved", "link created"):
            print("  {} - {} - {}".format(entry["severity_text"], entry["body"], attr_or_body(entry)))
            n += 1
    if n == 0:
        print("  (no worker logs yet - run generate_traffic.sh first)")
    print()


def attr_or_body(entry: dict):
    a = entry.get("attributes")
    return a if a else entry.get("body")


def works_trace_tree() -> None:
    print("== Works: cross-worker trace waterfall (engine::traces::tree) ==")
    listed = trig("engine::traces::list", {"name": "GET /s/:code", "limit": 1})
    if not listed["spans"]:
        print("  (no GET /s/:code spans yet - run generate_traffic.sh first)")
        return
    tid = listed["spans"][0]["trace_id"]
    print("  trace_id={}".format(tid))
    tree = trig("engine::traces::tree", {"trace_id": tid})
    roots = extract_roots(tree)
    if not roots:
        print("  (unexpected tree shape):")
        print(json.dumps(tree, indent=2)[:800])
        return
    for root in roots:
        walk(root)
    print()


def extract_roots(tree: dict) -> list:
    """The tree comes back as {"roots": [<node>, ...]}; tolerate other shapes."""
    if isinstance(tree, dict):
        if isinstance(tree.get("roots"), list):
            return tree["roots"]
        if "name" in tree:
            return [tree]
        for key in ("root", "tree", "span"):
            node = tree.get(key)
            if isinstance(node, dict) and "name" in node:
                return [node]
    return []


def walk(node: dict, depth: int = 0) -> None:
    print(
        "  {}{} ({}) {:.3f} ms status={}".format(
            "  " * depth, node["name"], node.get("service_name"), dur_ms(node), node.get("status")
        )
    )
    for child in node.get("children", []):
        walk(child, depth + 1)


if __name__ == "__main__":
    issue1_span_status()
    issue2_duration_sort()
    works_logs()
    works_trace_tree()
