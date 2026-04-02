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
  const account = await iii.trigger({
    function_id: "accounts-service::set-status",
    payload: {
      accountId,
      status: request.body.status,
    },
  });
  await iii.trigger({
    function_id: "state::set",
    payload: {
      scope: "accounts",
      key: accountId,
      value: {
        _key: accountId,
        ...account,
      },
    },
  });
  logger.info("reactive.status_update", {
    accountId,
    status: account.status,
  });
  return { accountId, status: account.status };
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
  logger.info("reactive.state_to_pubsub", {
    accountId: update.accountId ?? update.id,
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
