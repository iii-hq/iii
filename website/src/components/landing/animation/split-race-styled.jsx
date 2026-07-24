// Split Race — Styled. Same 8-step race, but each side wears its real product
// chrome: opencode dark TUI (traditional, left) vs iii console (right).
// Vendored from the omelette animation project. Local deltas, marked `iii:`:
// the engine is imported for its side effect (it publishes on globalThis), the
// scene list and playback mode are inline literals instead of window globals
// set by the host document, and the component is a default export.
import './animations-v2.jsx'

const { SceneStage, useScene, clamp, Easing } = globalThis;
const MONO = "'Chivo Mono', ui-monospace, monospace";

// opencode dark palette (third-party product depiction)
const OC = { titlebar: "#2b303b", body: "#0d1117", band: "#161b22", text: "#c9d1d9", dim: "#6e7681", faint: "#484f58", blue: "#4a9eff", amber: "#d29922", rule: "#21262d" };

// ── content, accumulating by step (same story as Split Race) ────────────────
const III = [
  [{ k: "prompt", t: "build a payments ledger service with a durable db" }],
  [{ k: "muted", t: "Finding workers..." },
   { k: "ok", t: "found existing database worker", hl: "database" },
   { k: "muted", t: "I have all I need, building payments-ledger functions" }],
  [{ k: "fn", t: "payments::charge::record { amount, customer_id }", d: "Record an authorized charge" },
   { k: "fn", t: "payments::charge::refund { charge_id }", d: "Issue a full or partial refund" }],
  [{ k: "fn", t: "payments::webhook::stripe { event }", d: "Ingest a provider event, idempotent" },
   { k: "fn", t: "payments::ledger::reconcile { period }", d: "Reconcile the ledger for a period" }],
  [{ k: "muted", t: "Registering functions to http endpoint triggers [/charge/record, /charge/refund, /webhook/stripe, /ledger/reconcile]", hl: "http" },
   { k: "test", t: "Running tests on dev ... ", flash: "pass" },
   { k: "muted", t: "All tests pass. Building worker" }],
  [{ k: "worker", t: "payments-ledger" }],
  [{ k: "confirm", t: "Deploy to production? ", ans: "yes" },
   { k: "cmd", t: "compose::add ./payments-ledger --host production" },
   { k: "ok", t: "joined prod · 25 workers connected" }],
  [],
];
const TRAD = [
  [{ k: "prompt", t: "build a payments ledger service with a durable db" }],
  [{ k: "muted", t: "I'll research a stack. Searching for options." },
   { k: "tool", t: "Grep \"node orm postgres\"" },
   { k: "tool", t: "Web \"prisma vs drizzle vs typeorm\" (12,300 results)" },
   { k: "muted", t: "Reading blog posts to compare tradeoffs…" }],
  [{ k: "muted", t: "There are several ORMs. Which do you want?" },
   { k: "choice", t: ["prisma", "drizzle", "typeorm"] },
   { k: "ask", t: "Asking questions…" }],
  [{ k: "tool", t: "Bash \"npm install prisma @prisma/client\"" },
   { k: "warn", t: "ERESOLVE unable to resolve dependency tree" },
   { k: "muted", t: "Failed to install dependencies for prisma, trying plain postgres" },
   { k: "read", t: "Write src/db/schema.ts  (+190)" },
   { k: "read", t: "Edit src/api-gateway.ts  (+38)  register payments routes" }],
  [],
  [{ k: "muted", t: "Ready to test locally." },
   { k: "cmd", t: "npm run dev" }],
  [{ k: "cmd", t: "git rebase main" },
   { k: "muted", t: "Rebasing..." },
   { k: "muted", t: "Auto-merging src/api-gateway.ts" },
   { k: "warn", t: "CONFLICT (content): Merge conflict in src/api-gateway.ts" },
   { k: "muted", t: "Automatic rebase failed...resolving merge conflicts" }],
  [],
];

const TYPED = { prompt: 1, muted: 1, ok: 1, cmd: 1, warn: 1, del: 1, fn: 1, tool: 1, read: 1, ask: 1, test: 1, confirm: 1 };
const GREEN = "#2fae5a";
const TRAD_TOK = [8, 64, 102, 188, 188, 258, 336, 356]; // k tokens per step (left, discrete)
const DUR = [3, 4.5, 3.5, 4.5, 4, 3, 4, 3.5]; // must match OM_SCENES
// staggered think-pauses (fraction of the step) — sides alternate who moves first
const III_DELAY = [0, 0, 0.30, 0, 0.25, 0, 0, 0];
const TRAD_DELAY = [0, 0.28, 0, 0.25, 0, 0.25, 0.25, 0];
function fmtTime(s) { return Math.floor(s / 60) + ":" + String(Math.floor(s % 60)).padStart(2, "0"); }

// shared typing model — one real chars-per-second rate for both sides
const CPS = 70;
function typeSteps(data, step, progress, dur, delayFrac) {
  const out = [];
  const wait = (delayFrac || 0) * dur;
  const elapsed = progress * dur - wait;
  for (let g = 0; g <= step && g < data.length; g++) {
    const gl = data[g];
    if (g !== step) { gl.forEach(line => out.push({ line, chars: -1, caret: false, op: 1 })); continue; }
    if (elapsed <= 0) continue;
    const costs = gl.map(l => TYPED[l.k] ? String(l.t).length : 10);
    const total = costs.reduce((a, b) => a + b, 0) || 1;
    const head = Math.min(elapsed * CPS, total);
    let acc = 0;
    gl.forEach((line, li) => {
      const start = acc, end = acc + costs[li]; acc = end;
      if (head <= start) return;
      const isText = !!TYPED[line.k];
      let chars = -1, caret = false, op = 1;
      if (head < end) { if (isText) { chars = Math.floor(head - start); caret = true; } else { op = clamp((head - start) / costs[li], 0, 1); } }
      let flashAmt = 0;
      if ((line.flash || line.ans || line.hl) && head >= end) {
        flashAmt = clamp(1 - (elapsed - end / CPS) / 1.2, 0, 1);
      }
      out.push({ line, chars, caret, op, flashAmt });
    });
  }
  return out;
}
function isThinking(step, progress, dur, delayFrac) {
  return step > 0 && step < 7 && progress * dur < (delayFrac || 0) * dur;
}
function Thinking({ t, color }) {
  const n = 1 + (Math.floor(t * 2.6) % 3);
  return <div style={{ fontFamily: MONO, fontSize: 21, color, opacity: 0.85, letterSpacing: "-0.01em" }}>Thinking{".".repeat(n)}</div>;
}
const slice = (line, chars) => typeof line.t === "string" ? (chars < 0 ? line.t : line.t.slice(0, Math.max(0, chars))) : "";

// ══ OPENCODE WINDOW (traditional) ═══════════════════════════════════════════
function OCCaret() { return <span style={{ display: "inline-block", width: "0.55ch", height: "1.05em", background: OC.text, marginLeft: 1, transform: "translateY(3px)" }} />; }
function OCRow({ item }) {
  const { line, chars, caret, op } = item;
  const k = line.k, txt = slice(line, chars), car = caret ? <OCCaret /> : null;
  const base = { fontFamily: MONO, fontSize: 20, lineHeight: 1.6, letterSpacing: "-0.01em", opacity: op, whiteSpace: "pre-wrap" };
  if (k === "prompt")
    return <div style={{ background: "#1c2230", borderLeft: `2px solid ${OC.blue}`, padding: "12px 18px", margin: "2px 0 10px", ...base, color: OC.text, fontSize: 21 }}>{txt}{car}</div>;
  if (k === "choice")
    return <div style={{ ...base, color: OC.dim, display: "flex", gap: 22 }}>{line.t.map((c, i) => <span key={i}>[ ] {c}</span>)}</div>;
  if (k === "bar") {
    const n = Math.round(line.t * 18);
    return <div style={{ ...base, color: OC.dim }}><span style={{ color: OC.faint }}>* </span>{"█".repeat(n) + "░".repeat(18 - n)}  {Math.round(line.t * 100)}%</div>;
  }
  const pre = k === "tool" ? "* " : k === "read" ? "→ " : k === "cmd" ? "→ " : k === "ask" ? "~ " : "";
  const color = k === "muted" ? OC.text : k === "warn" ? OC.amber : k === "ask" ? OC.text
    : k === "del" ? OC.faint : OC.dim;
  return <div style={{ ...base, color, textDecoration: k === "del" ? "line-through" : "none" }}>
    {pre ? <span style={{ color: k === "warn" ? OC.amber : OC.faint }}>{pre}</span> : null}{txt}{car}
  </div>;
}
function OpenCodeWindow({ step, progress, gl, dur }) {
  const delay = TRAD_DELAY[Math.min(step, TRAD_DELAY.length - 1)];
  const rows = typeSteps(TRAD, step, progress, dur, delay);
  const thinking = isThinking(step, progress, dur, delay);
  const tk = TRAD_TOK[Math.min(step, TRAD_TOK.length - 1)];
  const tokens = tk * 1000;
  const pct = Math.min(96, Math.round(tk / 4));
  const cost = (tk * 0.011).toFixed(2);
  return (
    <div style={{ position: "absolute", left: 40, top: 52, width: 880, bottom: 52, display: "flex", flexDirection: "column", background: OC.body, border: `1px solid #000`, boxShadow: "0 24px 60px rgba(0,0,0,0.28)", overflow: "hidden" }}>
      {/* titlebar */}
      <div style={{ height: 46, background: OC.titlebar, display: "flex", alignItems: "center", gap: 10, padding: "0 18px", flexShrink: 0 }}>
        <span style={{ width: 13, height: 13, borderRadius: 13, background: "#ff5f57" }} />
        <span style={{ width: 13, height: 13, borderRadius: 13, background: "#febc2e" }} />
        <span style={{ width: 13, height: 13, borderRadius: 13, background: "#28c840" }} />
        <span style={{ marginLeft: 14, fontFamily: MONO, fontSize: 17, color: "#aeb6c2", fontWeight: 600 }}>▪ OC | payments ledger service in repo …</span>
      </div>
      {/* header band */}
      <div style={{ background: OC.band, borderTop: `1px solid ${OC.rule}`, borderBottom: `1px solid ${OC.rule}`, padding: "16px 22px", display: "flex", alignItems: "center", flexShrink: 0 }}>
        <span style={{ marginLeft: "auto", fontFamily: MONO, fontSize: 18, color: OC.dim, fontVariantNumeric: "tabular-nums" }}>{tokens.toLocaleString()}  {pct}% (${cost})</span>
      </div>
      {/* transcript */}
      <div style={{ flex: 1, padding: "20px 22px", display: "flex", flexDirection: "column", justifyContent: "flex-end", gap: 6, overflow: "hidden", minHeight: 0 }}>
        {rows.map((item, i) => <OCRow key={i} item={item} />)}
        {thinking && <Thinking t={progress * dur} color={OC.dim} />}
      </div>
      {/* input band */}
      <div style={{ background: OC.band, borderTop: `1px solid ${OC.rule}`, padding: "16px 22px", flexShrink: 0 }}>
        <span style={{ display: "inline-block", width: 11, height: 22, background: OC.text, transform: "translateY(4px)" }} />
      </div>
      <div style={{ background: OC.band, borderTop: `1px solid ${OC.rule}`, padding: "10px 22px 8px", flexShrink: 0, fontFamily: MONO, fontSize: 17 }}>
        <span style={{ color: OC.blue }}>Build</span> <span style={{ color: OC.text, marginLeft: 12 }}>Claude Sonnet 5</span>
      </div>
      <div style={{ background: OC.body, padding: "8px 22px 12px", flexShrink: 0, display: "flex", fontFamily: MONO, fontSize: 15, color: OC.faint }}>
        <span>· · · · ·  <span style={{ color: OC.dim }}>esc</span> interrupt</span>
        <span style={{ marginLeft: "auto" }}><span style={{ color: OC.dim }}>ctrl+t</span> variants&nbsp;&nbsp;<span style={{ color: OC.dim }}>tab</span> agents&nbsp;&nbsp;<span style={{ color: OC.dim }}>ctrl+p</span> commands</span>
      </div>
    </div>
  );
}

// ══ iii CONSOLE WINDOW ══════════════════════════════════════════════════════
function IiiCaret() { return <span style={{ display: "inline-block", width: "0.55ch", height: "1em", background: "var(--ink)", marginLeft: 1, transform: "translateY(3px)" }} />; }
// iii: `on` is the latch — a worker the agent has used stays filled orange with
// white text for the rest of the run; the flash only drives the glow.
function Chip({ label, hl, on }) {
  const amt = hl || 0;
  return <span style={{ border: `1px solid ${on || amt > 0 ? "var(--accent)" : "var(--rule)"}`, padding: "5px 12px", fontSize: 18, color: on ? "#fff" : "var(--ink)", background: on ? "var(--accent)" : (amt > 0 ? `color-mix(in oklab, var(--accent) ${28 * amt}%, var(--paper-2))` : "var(--paper-2)"), boxShadow: amt > 0 ? `0 0 ${20 * amt}px color-mix(in oklab, var(--accent) ${70 * amt}%, transparent)` : "none", display: "inline-flex", alignItems: "center", gap: 8, fontFamily: MONO }}>
    {label}
  </span>;
}
function IiiRow({ item }) {
  const { line, chars, caret, op, flashAmt } = item;
  const k = line.k, txt = slice(line, chars), car = caret ? <IiiCaret /> : null;
  const base = { fontFamily: MONO, fontSize: 22, lineHeight: 1.5, letterSpacing: "-0.01em", opacity: op, whiteSpace: "pre-wrap" };
  const amt = flashAmt || 0;
  const flashBox = (label) => <span style={{ marginLeft: 8, color: GREEN, fontWeight: 600, padding: "1px 9px", border: `1px solid ${GREEN}`, background: `color-mix(in oklab, ${GREEN} ${20 * amt}%, transparent)`, boxShadow: amt > 0 ? `0 0 ${18 * amt}px color-mix(in oklab, ${GREEN} ${75 * amt}%, transparent)` : "none" }}>{label}</span>;
  if (k === "prompt")
    return <div style={{ ...base, color: "var(--ink)", fontWeight: 500 }}><span style={{ color: "var(--ink-ghost)" }}>$ </span>{txt}{car}</div>;
  if (k === "chips")
    return <div style={{ display: "flex", gap: 10, flexWrap: "wrap", opacity: op }}>{line.t.map((c, i) => <Chip key={i} label={c} />)}</div>;
  if (k === "fn")
    return <div style={{ display: "flex", border: "1px solid var(--rule)", background: "var(--paper)", opacity: op }}>
      <div style={{ flex: 1, padding: "9px 14px", ...base, opacity: 1, color: "var(--ink-2)", fontSize: 20 }}>{txt}{car}</div>
      <div style={{ width: "44%", padding: "9px 14px", borderLeft: "1px solid var(--rule)", ...base, opacity: 1, color: "var(--ink-faint)", fontSize: 19 }}>{line.d}</div>
    </div>;
  if (k === "worker")
    return <div style={{ border: "1px solid var(--rule)", background: "var(--panel)", padding: "12px 16px", display: "flex", alignItems: "center", gap: 14, opacity: op }}>
      <span style={{ width: 9, height: 9, borderRadius: 9, background: "var(--accent)" }} />
      <span style={{ ...base, opacity: 1, color: "var(--ink)", fontSize: 22 }}>{line.t}</span>
      <span style={{ ...base, opacity: 1, color: "var(--ink-faint)", fontSize: 18 }}>node</span>
      <span style={{ marginLeft: "auto", ...base, opacity: 1, color: "var(--ink-faint)", fontSize: 16, letterSpacing: "0.1em" }}>STANDALONE</span>
    </div>;
  if (k === "test")
    return <div style={{ ...base, color: "var(--ink-faint)", display: "flex", alignItems: "center" }}>{txt}{car}{chars < 0 && flashBox(line.flash)}</div>;
  if (k === "confirm")
    return <div style={{ ...base, color: "var(--ink)", display: "flex", alignItems: "center" }}>{txt}{car}{chars < 0 && flashBox(line.ans)}</div>;
  if (k === "ok")
    return <div style={{ ...base, color: "var(--ink)" }}>{txt}{car}</div>;
  return <div style={{ ...base, color: "var(--ink-faint)" }}>{k === "cmd" ? <span style={{ color: "var(--ink-ghost)" }}>{"› "}</span> : null}{txt}{car}</div>;
}
function WorkersBar({ step, progress, hl, on }) {
  const base = ["http", "state", "database", "queue", "storage"];
  const adding = step === 6;
  const added = step > 6;
  const p = adding ? Easing.easeOutCubic(clamp(progress / 0.5, 0, 1)) : 0;
  return (
    <div style={{ background: "var(--paper)", borderBottom: "1px solid var(--rule)", padding: "12px 20px", display: "flex", alignItems: "center", gap: 10, flexShrink: 0, flexWrap: "wrap" }}>
      <span style={{ fontFamily: MONO, fontSize: 13, letterSpacing: "0.12em", color: "var(--ink-ghost)", marginRight: 4 }}>WORKERS</span>
      {base.map((c, i) => <Chip key={i} label={c} hl={hl && hl.label === c ? hl.amt : 0} on={on.includes(c)} />)}
      {(adding || added) && (
        <span style={{ display: "inline-flex", opacity: added ? 1 : p, transform: `scale(${added ? 1 : 0.7 + 0.3 * p})`, transformOrigin: "left center" }}>
          <span style={{ border: "1px solid var(--accent)", padding: "5px 12px", fontSize: 18, color: "#fff", background: "var(--accent)", display: "inline-flex", alignItems: "center", gap: 8, fontFamily: MONO, boxShadow: adding ? `0 0 0 ${6 * (1 - p)}px color-mix(in oklab, var(--accent) 30%, transparent)` : "none" }}>
            payments-ledger
          </span>
        </span>
      )}
    </div>
  );
}
function IiiWindow({ step, progress, gl, dur }) {
  const delay = III_DELAY[Math.min(step, III_DELAY.length - 1)];
  const rows = typeSteps(III, step, progress, dur, delay);
  const thinking = isThinking(step, progress, dur, delay);
  const hlRow = rows.find(r => r.line.hl && r.flashAmt > 0);
  const hl = hlRow ? { label: hlRow.line.hl, amt: hlRow.flashAmt } : null;
  // iii: latched workers, derived from the clock (chars === -1 means the line
  // that names the worker has finished typing) so scrubbing stays exact.
  const onWorkers = rows.filter(r => r.line.hl && r.chars === -1).map(r => r.line.hl);
  const iiiTok = Math.round(4 + gl * 7);
  const pct = Math.round(6 + gl * 2);
  return (
    <div style={{ position: "absolute", left: 960, top: 52, width: 920, bottom: 52, display: "flex", flexDirection: "column", background: "var(--bg)", border: "1px solid var(--rule)", boxShadow: "0 24px 60px rgba(0,0,0,0.12)", overflow: "hidden" }}>
      {/* top workers/agent bar */}
      <div style={{ height: 46, background: "var(--paper)", borderBottom: "1px solid var(--rule)", display: "flex", alignItems: "center", gap: 14, padding: "0 20px", flexShrink: 0 }}>
        <span style={{ fontFamily: MONO, fontSize: 18, color: "var(--ink)", letterSpacing: "0.04em" }}>{">"} iii</span>
        <span style={{ fontFamily: MONO, fontSize: 14, letterSpacing: "0.14em", color: "var(--ink-ghost)" }}>CONSOLE</span>
      </div>
      {/* agent status row */}
      <div style={{ background: "var(--paper)", borderBottom: "1px solid var(--rule)", display: "flex", alignItems: "center", gap: 12, padding: "12px 20px", flexShrink: 0 }}>
        <div style={{ marginLeft: "auto", display: "flex", alignItems: "center", gap: 14, fontFamily: MONO, fontSize: 20, color: "var(--ink-faint)" }}>
          <span style={{ letterSpacing: "0.1em", color: "var(--ink-ghost)" }}>CTX</span>
          <span style={{ width: 110, height: 13, border: "1px solid var(--rule)", position: "relative", display: "inline-block" }}>
            <span style={{ position: "absolute", inset: 0, width: pct + "%", background: "var(--ink-ghost)" }} />
          </span>
          <span style={{ fontVariantNumeric: "tabular-nums" }}>{pct}%  {iiiTok}k/1.0M</span>
          <span style={{ display: "flex", alignItems: "center", gap: 8, marginLeft: 6 }}><span style={{ width: 10, height: 10, borderRadius: 10, background: GREEN }} />CONNECTED</span>
        </div>
      </div>
      <WorkersBar step={step} progress={progress} hl={hl} on={onWorkers} />
      {/* transcript */}
      <div style={{ flex: 1, padding: "22px 22px", display: "flex", flexDirection: "column", justifyContent: "flex-end", gap: 11, overflow: "hidden", minHeight: 0 }}>
        {rows.map((item, i) => <IiiRow key={i} item={item} />)}
        {thinking && <Thinking t={progress * dur} color="var(--ink-faint)" />}
      </div>
      {/* trigger bar */}
      <div style={{ borderTop: "1px solid var(--rule)", background: "var(--paper)", padding: "10px 20px", flexShrink: 0, display: "flex", alignItems: "center", gap: 10, fontFamily: MONO, fontSize: 15, color: "var(--ink-faint)" }}>
        <span style={{ color: "var(--accent)" }}>⚡</span> {step >= 6 ? "2" : "1"} trigger registered
      </div>
      {/* input */}
      <div style={{ padding: "14px 20px 16px", flexShrink: 0, display: "flex", flexDirection: "column", gap: 12 }}>
        <div style={{ border: "1px solid var(--rule)", background: "var(--paper)", padding: "14px 16px", fontFamily: MONO, fontSize: 16, color: "var(--ink-ghost)" }}>send a message…</div>
        <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
          <span style={{ border: "1px solid var(--rule)", padding: "6px 12px", fontFamily: MONO, fontSize: 14, color: "var(--ink)", display: "flex", gap: 8, alignItems: "center" }}>⇄ agent ▾</span>
          <span style={{ border: "1px solid var(--rule)", padding: "6px 12px", fontFamily: MONO, fontSize: 14, color: "var(--ink-faint)", flex: 1 }}>claude sonnet 5 ▾</span>
          <span style={{ border: "1px solid var(--ink)", width: 34, height: 30, display: "flex", alignItems: "center", justifyContent: "center", fontSize: 16, color: "var(--ink)" }}>↑</span>
        </div>
      </div>
    </div>
  );
}

// ══ stage ═══════════════════════════════════════════════════════════════════
function World() {
  const { scene, progress } = useScene();
  const step = scene.step;
  const gl = step + progress;
  const outro = step >= 7 ? Easing.easeOutCubic(clamp(progress / 0.6, 0, 1)) : 0;
  const dur = DUR[Math.min(step, DUR.length - 1)];
  return (
    <div style={{ position: "absolute", inset: 0, background: "var(--panel)", fontFamily: MONO }}>
      <OpenCodeWindow step={step} progress={progress} gl={gl} dur={dur} />
      <IiiWindow step={step} progress={progress} gl={gl} dur={dur} />
      {/* outro overlays */}
      <div style={{ position: "absolute", left: 40, top: 52, width: 880, bottom: 52, display: "flex", alignItems: "center", justifyContent: "center", opacity: outro, background: "rgba(13,17,23,0.74)", pointerEvents: "none" }}>
        <span style={{ fontFamily: MONO, fontSize: 44, color: "#e6edf3", letterSpacing: "-0.02em" }}>Still developing</span>
      </div>
      <div style={{ position: "absolute", left: 960, top: 52, width: 920, bottom: 52, display: "flex", alignItems: "center", justifyContent: "center", gap: 16, opacity: outro, background: "color-mix(in oklab, var(--bg) 80%, transparent)", pointerEvents: "none" }}>
        <span style={{ fontFamily: MONO, fontSize: 44, color: "var(--ink)", letterSpacing: "-0.02em" }}>Deployed to production</span>
      </div>
    </div>
  );
}
// iii: from the host document's OM_SCENES / OM_PLAYBACK inline scripts.
const SCENES = '[{"name":"Prompt","dur":1.6,"step":0,"nat":3},{"name":"Find","dur":3,"step":1,"nat":4.5},{"name":"Early","dur":3.5,"step":2},{"name":"Iterate","dur":4.5,"step":3},{"name":"Test","dur":4,"step":4},{"name":"Worker","dur":3,"step":5},{"name":"Ship","dur":4,"step":6},{"name":"Result","dur":5.1,"step":7,"nat":3.5}]';
const PLAYBACK = '{"mode":"loop"}';

export default function SplitRaceStyled() {
  const map = {}; ["Prompt", "Find", "Early", "Iterate", "Test", "Worker", "Ship", "Result"].forEach(n => map[n] = World);
  return <SceneStage width={1920} height={1200} bg="var(--panel)" scenes={SCENES} playback={PLAYBACK}>{map}</SceneStage>;
}
