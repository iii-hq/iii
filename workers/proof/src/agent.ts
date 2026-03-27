import { SYSTEM_PROMPT, buildUserPrompt } from "./prompt.js";
import { getAnthropicTools, toolNameToFunctionId } from "./tools.js";
import type { StepResult, RunReport } from "./types.js";
import type { CoverageReport } from "./context.js";

const MAX_ITERATIONS = 50;

const STEP_MARKER_RE =
  /^(STEP_START|STEP_DONE|ASSERTION_PASSED|ASSERTION_FAILED|RUN_COMPLETED)\|([^|]+)\|(.+)$/gm;

type IIITrigger = (req: { function_id: string; payload: unknown }) => Promise<unknown>;

export async function runAgent(
  trigger: IIITrigger,
  diff: string,
  files: string[],
  baseUrl: string,
  runId: string,
  instruction?: string,
  commits?: Array<{ hash: string; subject: string }>,
  coverage?: CoverageReport,
): Promise<RunReport> {
  if (!process.env.ANTHROPIC_API_KEY) {
    throw new Error(
      "ANTHROPIC_API_KEY required for automated runs. " +
      "For interactive testing, use Claude Code or Codex directly — " +
      "browser tools are registered as iii functions (proof::browser::*)."
    );
  }
  const { default: Anthropic } = await import("@anthropic-ai/sdk");
  const anthropic = new Anthropic();
  const startedAt = Date.now();
  const steps: StepResult[] = [];
  let runTitle = "Proof run";
  let runStatus: "pass" | "fail" | "error" = "pass";
  const recordedActions: Array<{ tool: string; input: Record<string, unknown> }> = [];

  const messages: any[] = [
    {
      role: "user",
      content: buildUserPrompt(diff, files, baseUrl, instruction, commits, coverage),
    },
  ];

  for (let iteration = 0; iteration < MAX_ITERATIONS; iteration++) {
    const response = await anthropic.messages.create({
      model: "claude-sonnet-4-20250514",
      max_tokens: 4096,
      system: SYSTEM_PROMPT,
      tools: getAnthropicTools() as any[],
      messages,
    });

    const toolResults: any[] = [];

    for (const block of response.content) {
      if (block.type === "text") {
        parseStepMarkers(block.text, steps);

        const runMatch = block.text.match(/RUN_COMPLETED\|(passed|failed)\|(.+)/);
        if (runMatch) {
          runStatus = runMatch[1] === "passed" ? "pass" : "fail";
          runTitle = runMatch[2].trim();
        }
      }

      if (block.type === "tool_use") {
        const fnId = toolNameToFunctionId(block.name);
        recordedActions.push({
          tool: block.name,
          input: block.input as Record<string, unknown>,
        });

        try {
          const result = await trigger({
            function_id: fnId,
            payload: block.input,
          });

          const isScreenshot = block.name === "browser_screenshot";
          if (isScreenshot && typeof result !== "string") {
            throw new Error("Screenshot returned invalid data");
          }
          toolResults.push({
            type: "tool_result",
            tool_use_id: block.id,
            content: isScreenshot
              ? [{ type: "image", source: { type: "base64", media_type: "image/png", data: result as string } }]
              : [{ type: "text", text: typeof result === "string" ? result : JSON.stringify(result) }],
          } as any);
        } catch (err: unknown) {
          const errMsg = err instanceof Error ? err.message : String(err);
          toolResults.push({
            type: "tool_result",
            tool_use_id: block.id,
            content: [{ type: "text", text: `Error: ${errMsg}` }],
            is_error: true,
          } as any);
        }
      }
    }

    await pushStepProgress(trigger, runId, steps);

    if (response.stop_reason === "end_turn") break;

    if (toolResults.length > 0) {
      messages.push({ role: "assistant", content: response.content as any[] });
      messages.push({ role: "user", content: toolResults });
    } else {
      break;
    }
  }

  if (steps.length === 0 && recordedActions.length > 0) {
    steps.push({
      id: "step-01",
      description: "Browser test execution",
      status: runStatus === "pass" ? "passed" : "failed",
      assertions: [],
      startedAt,
      completedAt: Date.now(),
    });
  }

  const passed = steps.filter((s) => s.status === "passed").length;
  const total = steps.length;

  return {
    runId,
    title: runTitle,
    steps,
    status: runStatus,
    passRate: total > 0 ? Math.round((passed / total) * 100) : 0,
    files,
    startedAt,
    completedAt: Date.now(),
    recordedActions,
  };
}

function parseStepMarkers(text: string, steps: StepResult[]): void {
  STEP_MARKER_RE.lastIndex = 0;
  let match: RegExpExecArray | null;
  while ((match = STEP_MARKER_RE.exec(text)) !== null) {
    const [, marker, id, detail] = match;

    switch (marker) {
      case "STEP_START":
        steps.push({ id, description: detail, status: "running", assertions: [], startedAt: Date.now() });
        break;
      case "STEP_DONE": {
        const step = steps.find((s) => s.id === id);
        if (step) {
          if (step.status !== "failed") step.status = "passed";
          step.completedAt = Date.now();
        }
        break;
      }
      case "ASSERTION_PASSED":
        steps.find((s) => s.id === id)?.assertions.push({ text: detail, passed: true });
        break;
      case "ASSERTION_FAILED": {
        const step = steps.find((s) => s.id === id);
        if (step) { step.status = "failed"; step.assertions.push({ text: detail, passed: false }); step.completedAt = Date.now(); }
        break;
      }
    }
  }
}

async function pushStepProgress(
  trigger: IIITrigger,
  runId: string,
  steps: StepResult[],
): Promise<void> {
  if (steps.length === 0) return;
  try {
    await trigger({
      function_id: "stream::set",
      payload: {
        stream_name: "proof",
        group_id: runId,
        item_id: `progress`,
        data: { steps, updatedAt: Date.now() },
      },
    });
  } catch {
    // stream push is best-effort
  }
}
