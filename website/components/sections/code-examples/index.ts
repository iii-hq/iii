import traditionalApi from "./traditional/api.ts?raw";
import traditionalPubsub from "./traditional/pubsub.ts?raw";
import traditionalRealtime from "./traditional/realtime.ts?raw";
import traditionalState from "./traditional/state.ts?raw";
import traditionalCron from "./traditional/cron.ts?raw";
import traditionalWorkflow from "./traditional/workflow.ts?raw";
import traditionalAiAgents from "./traditional/ai-agents.ts?raw";
import traditionalFeatureFlags from "./traditional/feature-flags.ts?raw";
import traditionalEtl from "./traditional/etl.ts?raw";
import traditionalReactive from "./traditional/reactive.ts?raw";

import iiiApi from "./iii/api.ts?raw";
import iiiPubsub from "./iii/pubsub.ts?raw";
import iiiRealtime from "./iii/realtime.ts?raw";
import iiiState from "./iii/state.ts?raw";
import iiiCron from "./iii/cron.ts?raw";
import iiiWorkflow from "./iii/workflow.ts?raw";
import iiiAiAgents from "./iii/ai-agents.ts?raw";
import iiiFeatureFlags from "./iii/feature-flags.ts?raw";
import iiiEtl from "./iii/etl.ts?raw";
import iiiReactive from "./iii/reactive.ts?raw";

export const traditional = {
  api: traditionalApi,
  pubsub: traditionalPubsub,
  realtime: traditionalRealtime,
  state: traditionalState,
  cron: traditionalCron,
  workflow: traditionalWorkflow,
  "ai-agents": traditionalAiAgents,
  "feature-flags": traditionalFeatureFlags,
  etl: traditionalEtl,
  reactive: traditionalReactive,
};

export const iii = {
  api: iiiApi,
  pubsub: iiiPubsub,
  realtime: iiiRealtime,
  state: iiiState,
  cron: iiiCron,
  workflow: iiiWorkflow,
  "ai-agents": iiiAiAgents,
  "feature-flags": iiiFeatureFlags,
  etl: iiiEtl,
  reactive: iiiReactive,
};
