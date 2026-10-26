#!/usr/bin/env node
// Controlled GET/POST probe for HTTP request-body channel retention.
// Usage: node request-body-probe.mjs G|P --seconds 360 --rate 20 [--body-bytes 100] [--out file]
import fs from 'node:fs';
import { performance } from 'node:perf_hooks';

const arg = (name, fallback) => {
  const i = process.argv.indexOf(`--${name}`);
  return i >= 0 ? process.argv[i + 1] : fallback;
};
const mode = process.argv[2];
if (!['G', 'P'].includes(mode)) throw new Error('mode must be G or P');
const seconds = Number(arg('seconds', '360'));
const rate = Number(arg('rate', '20'));
const bodyBytes = Number(arg('body-bytes', mode === 'P' ? '100' : '0'));
const out = arg('out', '');

const url = 'http://127.0.0.1:3711/probe';
const base = JSON.stringify({ query: 'mem-probe', limit: 1, pad: '' });
if (mode === 'P' && bodyBytes < Buffer.byteLength(base)) throw new Error('body-bytes too small');
const body = mode === 'P'
  ? JSON.stringify({ query: 'mem-probe', limit: 1, pad: 'x'.repeat(bodyBytes - Buffer.byteLength(base)) })
  : undefined;
if (body && Buffer.byteLength(body) !== bodyBytes) throw new Error(`body size mismatch: ${Buffer.byteLength(body)} != ${bodyBytes}`);
const durationMs = seconds * 1000;
const intervalMs = 1000 / rate;
const start = performance.now();
let sent = 0, ok = 0, errors = 0, active = 0, maxActive = 0;
const latency = [];
const request = async () => {
  const t0 = performance.now();
  try {
    const res = await fetch(url, {
      method: mode === 'G' ? 'GET' : 'POST',
      headers: mode === 'P' ? { 'content-type': 'application/json' } : {},
      body,
      signal: AbortSignal.timeout(30000),
    });
    await res.arrayBuffer();
    if (res.status >= 200 && res.status < 300) ok++; else errors++;
  } catch { errors++; }
  latency.push(performance.now() - t0);
  active--;
};
while (performance.now() - start < durationMs) {
  // Once the deadline is reached, stop admitting requests even while the
  // configured concurrency is saturated; then drain the in-flight set below.
  if (active >= 8) {
    await new Promise((resolve) => setTimeout(resolve, 1));
    continue;
  }
  const scheduled = start + sent * intervalMs;
  if (performance.now() < scheduled) {
    await new Promise((resolve) => setTimeout(resolve, Math.min(10, scheduled - performance.now())));
    continue;
  }
  sent++; active++; maxActive = Math.max(maxActive, active); void request();
}
while (active) await new Promise((resolve) => setTimeout(resolve, 5));
latency.sort((a, b) => a - b);
const q = (p) => latency.length ? latency[Math.min(latency.length - 1, Math.floor(p * (latency.length - 1)))] : null;
const result = {
  mode, seconds, rate, bodyBytes: body ? Buffer.byteLength(body) : 0,
  sent, ok, errors, achievedRate: sent / (performance.now() - start) * 1000,
  maxConcurrency: maxActive, latencyMs: { p50: q(0.5), p95: q(0.95), p99: q(0.99), max: q(1) },
};
const line = `${JSON.stringify(result)}\n`;
if (out) fs.appendFileSync(out, line); else process.stdout.write(line);
if (errors) process.exitCode = 1;
