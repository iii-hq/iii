import { registerWorker, Logger, TriggerAction } from "iii-sdk";

const iii = registerWorker(
  process.env.III_ENGINE_URL || "ws://localhost:49134",
  {
    workerName: "reactive-iii",
  },
);

iii.registerFunction({ id: "accounts::set-status" }, async (request: any) => {
  const logger = new Logger();
  const accountId = request.params.accountId;
  const status = String(request.body.status);
  await iii.trigger({
    function_id: "state::set",
    payload: {
      scope: "accounts",
      key: accountId,
      value: {
        _key: accountId,
        id: accountId,
        status,
        updatedAt: new Date().toISOString(),
      },
    },
  });
  logger.info("reactive.status_update", {
    accountId,
    status,
  });
  return { accountId, status };
});

iii.registerFunction({ id: "accounts::get" }, async (request: any) => {
  const account = await iii.trigger({
    function_id: "state::get",
    payload: {
      scope: "accounts",
      key: request.params.accountId,
    },
  });
  if (!account) {
    const error = new Error("Account not found") as Error & {
      status: number;
    };
    error.status = 404;
    throw error;
  }
  return account;
});

iii.registerFunction({ id: "accounts::on-change" }, async (event: any) => {
  const logger = new Logger();
  const update = event.new_value;
  iii.trigger({
    function_id: "publish",
    payload: {
      topic: "account_changes",
      data: update,
    },
    action: TriggerAction.Void(),
  });
  iii.trigger({
    function_id: "stream::send",
    payload: {
      stream_name: "account-changes",
      group_id: "global",
      id: `acc-${Date.now()}`,
      event_type: "account.updated",
      data: update,
    },
    action: TriggerAction.Void(),
  });
  logger.info("reactive.pubsub_to_stream", {
    accountId: update.id,
    status: update.status,
  });
  return { propagated: true };
});

iii.registerTrigger({
  type: "state",
  function_id: "accounts::on-change",
  config: { scope: "accounts" },
});

iii.registerTrigger({
  type: "http",
  function_id: "accounts::set-status",
  config: {
    api_path: "/reactive/accounts/:accountId/status",
    http_method: "POST",
  },
});

iii.registerTrigger({
  type: "http",
  function_id: "accounts::get",
  config: {
    api_path: "/reactive/accounts/:accountId",
    http_method: "GET",
  },
});
