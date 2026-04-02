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

export type Step = SlackMessageStep | ReplyStep | CodeEditorStep | StatusStep;

export type DemoMode = "hero" | "onboarding";

export interface DemoSequencerProps {
  steps: Step[];
  mode?: DemoMode;
  onComplete?: () => void;
  className?: string;
}
