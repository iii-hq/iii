export interface Sender {
  name: string;
  avatar?: string;
  role?: string;
}

export interface BaseStep {
  id: string;
  delay?: number;
  autoAdvance?: number;
}

export interface SlackMessageStep extends BaseStep {
  type: "slack-message";
  sender: Sender;
  content: string;
  action?: {
    label: string;
    count?: number;
  };
}

export interface ReplyStep extends BaseStep {
  type: "reply";
  content: string;
  typingSpeed?: number;
  sendLabel?: string;
}

export interface CodeEditorStep extends BaseStep {
  type: "code-editor";
  filename: string;
  language: string;
  lines: string[];
  lineDelay?: number;
  writeLabel?: string;
  doneLabel?: string;
}

export interface StatusStep extends BaseStep {
  type: "status";
  variant: "success" | "info" | "warn" | "alert" | "accent";
  icon?: "check" | "connect" | "deploy" | "observe" | "bolt";
  headline: string;
  detail?: string;
}

export interface TraceEntry {
  operation: string;
  duration?: string;
  status: "ok" | "error";
}

export interface SpanEntry {
  label: string;
  duration: string;
  /** 0–100, proportion of the parent trace duration */
  widthPercent: number;
  depth: number;
  status: "ok" | "error";
}

export interface SpanDetail {
  status: string;
  service: string;
  error?: string;
}

export interface ConsoleTraceStep extends BaseStep {
  type: "console-trace";
  traces: TraceEntry[];
  activeTraceIndex: number;
  spans: SpanEntry[];
  errorSpanIndex?: number;
  detail: SpanDetail;
}

export interface TerminalCommandStep extends BaseStep {
  type: "terminal-command";
  command: string;
  output: string;
  typingSpeed?: number;
}

export type Step =
  | SlackMessageStep
  | ReplyStep
  | CodeEditorStep
  | StatusStep
  | ConsoleTraceStep
  | TerminalCommandStep;

export type DemoMode = "hero" | "onboarding";

export interface DemoSequencerProps {
  steps: Step[];
  mode?: DemoMode;
  isDarkMode?: boolean;
  onComplete?: () => void;
  className?: string;
}
