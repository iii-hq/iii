// Tutorial configuration for the quickstart flow
// Each step defines a title and line ranges for comments and code
// You can safely ignore and delete this file if you're done with the quickstart

export type TutorialStepConfig = {
  id: string;
  title: string;
  comment: { start: number; end: number };
  code: { start: number; end: number };
};

export const tutorialConfig: TutorialStepConfig[] = [
  {
    id: "intro",
    title: "What are Steps?",
    comment: { start: 1, end: 2 },
    code: { start: 3, end: 4 },
  },
  {
    id: "config-object",
    title: "Config Object",
    comment: { start: 6, end: 7 },
    code: { start: 14, end: 14 },
  },
  {
    id: "trigger-types",
    title: "Types of Triggers",
    comment: { start: 9, end: 13 },
    code: { start: 23, end: 38 },
  },
  {
    id: "config-fields",
    title: "Required Fields",
    comment: { start: 15, end: 16 },
    code: { start: 17, end: 39 },
  },
  {
    id: "triggers-array",
    title: "Triggers Array",
    comment: { start: 19, end: 22 },
    code: { start: 23, end: 38 },
  },
  {
    id: "trigger-event",
    title: "Event Trigger",
    comment: { start: 20, end: 20 },
    code: { start: 24, end: 28 },
  },
  {
    id: "trigger-api",
    title: "API Trigger",
    comment: { start: 21, end: 22 },
    code: { start: 29, end: 33 },
  },
  {
    id: "trigger-cron",
    title: "Cron Trigger",
    comment: { start: 21, end: 22 },
    code: { start: 34, end: 37 },
  },
  {
    id: "config-optional",
    title: "Optional Fields",
    comment: { start: 41, end: 44 },
    code: { start: 45, end: 47 },
  },
  {
    id: "handler",
    title: "Handlers",
    comment: { start: 50, end: 53 },
    code: { start: 58, end: 64 },
  },
  {
    id: "done",
    title: "Done",
    comment: { start: 55, end: 57 },
    code: { start: 58, end: 64 },
  },
];
