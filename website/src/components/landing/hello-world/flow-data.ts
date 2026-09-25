import type { CodeLang } from "@/lib/shiki"

export type WorkerId = "ts" | "rs" | "py"

/** Inline result comments on the Node card, revealed as the flow resolves. */
export type CommentKey = "nums" | "pred" | "final"

export type WorkerSample = {
  id: WorkerId
  /** language tag shown in the window's title bar */
  tag: string
  /** the worker's role in the flow */
  role: string
  lang: CodeLang
  code: string
  /** result comments appended to a line (1-indexed) */
  comments?: { line: number; key: CommentKey; text: string }[]
}

export const WORKER_ORDER: WorkerId[] = ["ts", "rs", "py"]

export const WORKERS: Record<WorkerId, WorkerSample> = {
  ts: {
    id: "ts",
    tag: "Node.js",
    role: "Orchestrator",
    lang: "typescript",
    code: `import { registerWorker, Logger } from "iii-sdk"

const iii = registerWorker(
  "ws://localhost:49134"
)
const logger = new Logger()

const nums = await iii.trigger({
  function_id: "data::transform",
  payload: [1.0, 2.0, 3.0],
})

const pred = await iii.trigger({
  function_id: "ml::predict",
  payload: { data: nums },
})

logger.info(pred)`,
    comments: [
      { line: 11, key: "nums", text: "// → [2.0, 4.0, 6.0]" },
      { line: 16, key: "pred", text: "// → { predictions: […] }" },
      { line: 18, key: "final", text: "// → { predictions: [0.91, 0.07, 0.02] }" },
    ],
  },
  rs: {
    id: "rs",
    tag: "Rust",
    role: "Data transform",
    lang: "rust",
    code: `use iii_sdk::{register_worker, IIIError, InitOptions, Value};
use serde_json::json;

async fn transform(input: Value) -> Result<Value, IIIError> {
    let nums: Vec<f64> = serde_json::from_value(input)?;
    let result: Vec<f64> =
        nums.iter().map(|x| x * 2.0).collect();
    Ok(json!(result))
}

#[tokio::main]
async fn main() -> Result<(), IIIError> {
    let iii = register_worker(
        "ws://localhost:49134",
        InitOptions::default(),
    )?;
    iii.register_function("data::transform", transform);
    Ok(())
}`,
  },
  py: {
    id: "py",
    tag: "Python",
    role: "ML inference",
    lang: "python",
    code: `import torch
from iii import register_worker

iii = register_worker("ws://localhost:49134")

async def predict(input):
    t = torch.tensor(input["data"])
    result = model(t)
    return {
        "predictions": result.tolist()
    }

iii.register_function(
    "ml::predict", predict
)`,
  },
}

type LineMap = Partial<Record<WorkerId, number[]>>

export type FlowStep = {
  /** one-line description shown under the timeline */
  desc: string
  /** short name, used for keys and the segment's accessible name */
  label: string
  /** worker whose card is lit */
  active: WorkerId
  /** which of the two stacked workers sits on top */
  top: "rs" | "py"
  /** lines awaiting a result (dimmed) */
  pending?: LineMap
  /** lines executing right now (highlighted) */
  running?: LineMap
  /** Node result comments visible at this step (cumulative) */
  comments: CommentKey[]
  /** ms before autoplay advances */
  dwell: number
}

/**
 * The 7-step orchestration, in the Node code's execution order.
 * TS lines: 8-11 = `nums` trigger, 13-16 = `pred` trigger, 18 = logger.info.
 * RS lines 4-9 = transform(). PY lines 6-11 = predict().
 * Python comes to the top of the stack as soon as Rust's work resolves.
 */
export const FLOW_STEPS: FlowStep[] = [
  {
    desc: "Node triggers data::transform and awaits the result",
    label: "Node calls Rust",
    active: "ts",
    top: "rs",
    pending: { ts: [8, 9, 10, 11] },
    comments: [],
    dwell: 2400,
  },
  {
    desc: "Rust runs transform()",
    label: "Rust runs",
    active: "rs",
    top: "rs",
    running: { rs: [4, 5, 6, 7, 8, 9] },
    pending: { ts: [8, 9, 10, 11] },
    comments: [],
    dwell: 2400,
  },
  {
    desc: "Node's await resolves with nums = [2.0, 4.0, 6.0]",
    label: "nums resolved",
    active: "ts",
    top: "py",
    running: { ts: [8, 11] },
    comments: ["nums"],
    dwell: 2400,
  },
  {
    desc: "Node triggers ml::predict and awaits the result",
    label: "Node calls Python",
    active: "ts",
    top: "py",
    pending: { ts: [13, 14, 15, 16] },
    comments: ["nums"],
    dwell: 2400,
  },
  {
    desc: "Python runs predict()",
    label: "Python runs",
    active: "py",
    top: "py",
    running: { py: [6, 7, 8, 9, 10, 11] },
    pending: { ts: [13, 14, 15, 16] },
    comments: ["nums"],
    dwell: 2400,
  },
  {
    desc: "Node's await resolves with pred = { predictions: […] }",
    label: "pred resolved",
    active: "ts",
    top: "py",
    running: { ts: [13, 16] },
    comments: ["nums", "pred"],
    dwell: 2400,
  },
  {
    desc: "logger.info(pred) prints the result",
    label: "Result printed",
    active: "ts",
    top: "py",
    running: { ts: [18] },
    comments: ["nums", "pred", "final"],
    // the last step lingers so the printed result can be read
    dwell: 5200,
  },
]

export type LineState = "pending" | "running"

/** Line number → highlight state for one worker at one step. */
export function lineStates(step: FlowStep, worker: WorkerId): Map<number, LineState> {
  const states = new Map<number, LineState>()
  for (const n of step.pending?.[worker] ?? []) states.set(n, "pending")
  for (const n of step.running?.[worker] ?? []) states.set(n, "running")
  return states
}

export type WorkerStatus = "running" | "awaiting" | "idle"

/** What a worker is doing at a step, for the card's title-bar status. */
export function workerStatus(step: FlowStep, worker: WorkerId): WorkerStatus {
  if (step.running?.[worker]?.length) return "running"
  if (step.pending?.[worker]?.length) return "awaiting"
  return "idle"
}

/** The desktop layout (Node beside a Rust/Python stack) starts at this width. */
export const DESKTOP_MIN_WIDTH = 1024

/** Autoplay slows 3× on the one-card-per-step layout so captions can be read. */
export function dwellFor(index: number) {
  const base = FLOW_STEPS[index]?.dwell ?? 2400
  return window.matchMedia(`(max-width: ${DESKTOP_MIN_WIDTH - 1}px)`).matches ? base * 3 : base
}
