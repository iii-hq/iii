import traditionalApi from "./traditional/api.ts?raw";
import traditionalPubsub from "./traditional/pubsub.ts?raw";
import traditionalRealtime from "./traditional/realtime.ts?raw";
import traditionalState from "./traditional/state.ts?raw";
import traditionalCron from "./traditional/cron.ts?raw";
import traditionalLogging from "./traditional/logging.ts?raw";
import traditionalWorkflow from "./traditional/workflow.ts?raw";
import traditionalAiAgents from "./traditional/ai-agents.ts?raw";
import traditionalFeatureFlags from "./traditional/feature-flags.ts?raw";
import traditionalMultiplayer from "./traditional/multiplayer.ts?raw";
import traditionalEtl from "./traditional/etl.ts?raw";
import traditionalReactive from "./traditional/reactive.ts?raw";
import traditionalRemote from "./traditional/remote.ts?raw";

import iiiApi from "./iii/api.ts?raw";
import iiiPubsub from "./iii/pubsub.ts?raw";
import iiiRealtime from "./iii/realtime.ts?raw";
import iiiState from "./iii/state.ts?raw";
import iiiCron from "./iii/cron.ts?raw";
import iiiLogging from "./iii/logging.ts?raw";
import iiiWorkflow from "./iii/workflow.ts?raw";
import iiiAiAgents from "./iii/ai-agents.ts?raw";
import iiiFeatureFlags from "./iii/feature-flags.ts?raw";
import iiiMultiplayer from "./iii/multiplayer.ts?raw";
import iiiEtl from "./iii/etl.ts?raw";
import iiiReactive from "./iii/reactive.ts?raw";
import iiiRemote from "./iii/remote.ts?raw";

export const traditional = {
  api: traditionalApi,
  pubsub: traditionalPubsub,
  realtime: traditionalRealtime,
  state: traditionalState,
  cron: traditionalCron,
  logging: traditionalLogging,
  workflow: traditionalWorkflow,
  "ai-agents": traditionalAiAgents,
  "feature-flags": traditionalFeatureFlags,
  multiplayer: traditionalMultiplayer,
  etl: traditionalEtl,
  reactive: traditionalReactive,
  remote: traditionalRemote,
};

export const iii = {
  api: iiiApi,
  pubsub: iiiPubsub,
  realtime: iiiRealtime,
  state: iiiState,
  cron: iiiCron,
  logging: iiiLogging,
  workflow: iiiWorkflow,
  "ai-agents": iiiAiAgents,
  "feature-flags": iiiFeatureFlags,
  multiplayer: iiiMultiplayer,
  etl: iiiEtl,
  reactive: iiiReactive,
  remote: iiiRemote,
};
