import type { Sender, Step } from "./types";

const productOwner: Sender = {
  name: "[Requester Name]",
  role: "[Role]",
};

export const homepagePlaceholderFlow: Step[] = [
  {
    id: "msg-1",
    type: "slack-message",
    sender: productOwner,
    content:
      "[Slack-like request arrives here. Keep this as a placeholder prompt for your scenario.]",
    action: { label: "Open request" },
    delay: 500,
  },
  {
    id: "reply-1",
    type: "reply",
    content:
      "[Placeholder typed reply appears here. User clicks Send to continue the flow.]",
    sendLabel: "Send",
    delay: 250,
  },
  {
    id: "code-1",
    type: "code-editor",
    filename: "[placeholder-file.ts]",
    language: "typescript",
    lines: [
      "// [Placeholder instruction line 1]",
      "// [Placeholder instruction line 2]",
      "// [Placeholder instruction line 3]",
      "// [Placeholder instruction line 4]",
    ],
    writeLabel: "Write it",
    doneLabel: "Apply",
    delay: 300,
  },
  {
    id: "status-1",
    type: "status",
    variant: "info",
    icon: "connect",
    headline: "[Placeholder: connected/orchestrated]",
    detail: "[Placeholder: next capability unlocked after this interaction]",
    autoAdvance: 1400,
    delay: 200,
  },
  {
    id: "msg-2",
    type: "slack-message",
    sender: productOwner,
    content:
      "[Another incoming request shows that reusable functionality can be shared without knowing implementation details.]",
    action: { label: "Respond" },
    delay: 450,
  },
  {
    id: "reply-2",
    type: "reply",
    content:
      "[Placeholder confirmation reply. This can be replaced with any onboarding guidance text.]",
    sendLabel: "Send",
    delay: 220,
  },
  {
    id: "status-2",
    type: "status",
    variant: "success",
    icon: "check",
    headline: "[Placeholder: everything connected, observable, and live]",
    detail: "[Placeholder: finish state detail]",
    autoAdvance: 1600,
    delay: 250,
  },
];
