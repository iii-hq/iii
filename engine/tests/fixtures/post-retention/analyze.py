#!/usr/bin/env python3
"""Summarize RSS slopes before the first orphan-channel sweep (seconds 30..270)."""
import csv, json, pathlib, statistics, sys
print('| Run | Successful/sent | Robust KiB/req | RSS at load start/end (MiB) | RSS after idle (MiB) | Load min/max |')
print('|---|---:|---:|---:|---:|---:|')
for path in sorted(pathlib.Path(sys.argv[1]).glob('valid-*/probe.json')):
    p = json.loads(path.read_text())
    rows = [{k: float(v) for k,v in row.items()} for row in csv.DictReader((path.parent/'rss.csv').open())]
    groups = {}
    for r in rows:
        t = r['seconds']
        if 30 <= t <= 270: groups.setdefault(int(t//10),[]).append(r)
    points = [(statistics.median(r['seconds'] for r in rs), statistics.median(r['rss_kib'] for r in rs)) for rs in groups.values()]
    slopes = [(b[1]-a[1])/(b[0]-a[0])/p['rate'] for i,a in enumerate(points) for b in points[i+1:] if b[0]-a[0]>=30]
    slope = statistics.median(slopes)
    active = [r for r in rows if r['seconds'] <= p['seconds']]
    print(f"| {path.parent.name} | {p['ok']}/{p['sent']} | {slope:.3f} | {active[0]['rss_kib']/1024:.2f} / {active[-1]['rss_kib']/1024:.2f} | {rows[-1]['rss_kib']/1024:.2f} | {min(r['load1'] for r in rows):.2f}/{max(r['load1'] for r in rows):.2f} |")
