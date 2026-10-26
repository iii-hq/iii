#!/usr/bin/env python3
"""Execute isolated HTTP memory probes; never reuses an existing listener/process."""
import argparse, csv, json, os, pathlib, shutil, socket, subprocess, threading, time, urllib.request

p = argparse.ArgumentParser()
p.add_argument('--engine', required=True)
p.add_argument('--http', required=True)
p.add_argument('--node', default='node')
p.add_argument('--out', required=True)
p.add_argument('--seconds', type=int, default=360)
p.add_argument('--arena', choices=['2', 'unset'], required=True)
p.add_argument('--mode', choices=['G', 'P'], required=True)
p.add_argument('--idle', type=int, default=0)
a = p.parse_args()
fixture = pathlib.Path(__file__).resolve().parent
out = pathlib.Path(a.out).resolve()
out.mkdir(parents=True, exist_ok=False)
for port in (3711, 49734):
    with socket.socket() as s:
        s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        s.bind(('127.0.0.1', port))
assert shutil.disk_usage(out).free > 50 * 1024**3, 'need >50 GiB free'
subprocess.run(['df', '-h', '/'], stdout=(out/'host.txt').open('w'), check=True)
with (out/'host.txt').open('a') as f:
    subprocess.run(['uptime'], stdout=f, check=True)
# Equivalent to env -i: no worker identity, ambient config, credentials or OTel.
env = {'PATH': str(pathlib.Path(a.node).parent) + ':/usr/bin:/bin',
       'HOME': str(out), 'RUST_LOG': 'warn,iii::workers::worker::channels=info',
       'III_TELEMETRY_ENABLED': 'false', 'OTEL_ENABLED': 'false',
       'OTEL_LOGS_ENABLED': 'false', 'OTEL_METRICS_ENABLED': 'false', 'NO_COLOR': '1'}
if a.arena != 'unset': env['MALLOC_ARENA_MAX'] = a.arena
(out/'environment.json').write_text(json.dumps(env, indent=2))
# Current main's mandatory observability defaults override the legacy env vars.
# Seed its authoritative registry file before boot to isolate channel retention.
(out/'configuration').mkdir()
(out/'configuration'/'iii-observability.yaml').write_text('''id: iii-observability
name: Observability
description: Isolated channel-retention benchmark
value:
  enabled: false
  metrics_enabled: false
  logs_enabled: false
  level: warn
  trace_storage:
    enabled: false
''')
processes = []
stop = threading.Event()

def spawn(name, argv):
    proc = subprocess.Popen(argv, cwd=out, env=env, stdout=(out/f'{name}.log').open('w'), stderr=subprocess.STDOUT, start_new_session=True)
    processes.append(proc)
    (out/f'{name}.pid').write_text(str(proc.pid))
    return proc

def port_ready(port):
    for _ in range(300):
        if any(x.poll() is not None for x in processes): raise RuntimeError('process exited; see logs')
        try:
            with socket.create_connection(('127.0.0.1', port), timeout=.2): return
        except OSError: time.sleep(.1)
    raise TimeoutError(port)

def monitor(engine, t0):
    with (out/'rss.csv').open('w') as f:
        w = csv.writer(f); w.writerow(['seconds', 'rss_kib', 'anon_kib', 'fds', 'threads', 'load1'])
        while not stop.is_set():
            status = dict(x.split(':', 1) for x in pathlib.Path(f'/proc/{engine.pid}/status').read_text().splitlines())
            w.writerow([round(time.monotonic()-t0, 3), int(status['VmRSS'].split()[0]), int(status['RssAnon'].split()[0]),
                        len(list(pathlib.Path(f'/proc/{engine.pid}/fd').iterdir())), int(status['Threads']), os.getloadavg()[0]])
            f.flush(); stop.wait(1)

try:
    engine = spawn('engine', [str(pathlib.Path(a.engine).resolve()), '--config', str(fixture/'engine.yaml')])
    port_ready(49734)
    http = spawn('http', [str(pathlib.Path(a.http).resolve()), '--url', 'ws://127.0.0.1:49734', '--config', str(fixture/'http.yaml')])
    port_ready(3711)
    worker = spawn('worker', [a.node, str(fixture/'worker.mjs')])
    for _ in range(300):
        try:
            with urllib.request.urlopen('http://127.0.0.1:3711/probe', timeout=2) as r:
                assert r.status == 200
            break
        except Exception: time.sleep(.1)
    else: raise TimeoutError('route registration')
    subprocess.run([a.node, str(fixture/'check.mjs')], env=env, cwd=out,
                   stdout=(out/'check.log').open('w'), stderr=subprocess.STDOUT, timeout=40, check=True)
    time.sleep(2)
    t0 = time.monotonic()
    thread = threading.Thread(target=monitor, args=(engine,t0)); thread.start()
    subprocess.run([a.node, str(fixture/'probe.mjs'), a.mode, '--seconds', str(a.seconds), '--rate', '20',
                    '--body-bytes', '100', '--out', str(out/'probe.json')], env=env, cwd=out,
                   stdout=(out/'probe.log').open('w'), stderr=subprocess.STDOUT, timeout=a.seconds+60, check=True)
    time.sleep(a.idle)
    stop.set(); thread.join()
finally:
    stop.set()
    for proc in reversed(processes):
        proc.terminate()
    for proc in reversed(processes):
        try: proc.wait(timeout=10)
        except subprocess.TimeoutExpired: proc.kill(); proc.wait()
    with (out/'host.txt').open('a') as f: subprocess.run(['uptime'], stdout=f)
