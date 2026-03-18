import { registerWorker, Logger, TriggerAction } from "iii-sdk";

const iii = registerWorker(process.env.III_ENGINE_URL || "ws://localhost:49134", {
  workerName: "realtime-iii",
});

iii.registerFunction({ id: "rooms::join" }, async (request: any) => {
  const logger = new Logger();
  const roomId = request.body.roomId;
  const userId = request.body.userId;
  await iii.trigger({
    function_id: "state::set",
    payload: {
      scope: "room-presence",
      key: `${roomId}:${userId}`,
      value: { _key: `${roomId}:${userId}`, roomId, userId, joinedAt: new Date().toISOString() },
    },
  });
  const members = await iii.trigger({
    function_id: "state::list",
    payload: { scope: "room-presence" },
  });
  const count = members.filter((member: any) => member.roomId === roomId).length;
  iii.trigger({
    function_id: "stream::send",
    payload: {
      stream_name: "room-presence",
      group_id: roomId,
      id: `presence-${Date.now()}`,
      event_type: "presence.updated",
      data: { roomId, count },
    },
    action: TriggerAction.Void(),
  });
  logger.info("realtime.room_join", { roomId, userId, count });
  return { roomId, count };
});

iii.registerFunction({ id: "rooms::update-score" }, async (request: any) => {
  const logger = new Logger();
  const roomId = request.params.roomId;
  const update = {
    roomId,
    playerId: request.body.playerId,
    score: request.body.score,
    updatedAt: new Date().toISOString(),
  };
  await iii.trigger({
    function_id: "state::set",
    payload: {
      scope: "room-state",
      key: `${roomId}:${update.playerId}`,
      value: { _key: `${roomId}:${update.playerId}`, ...update },
    },
  });
  logger.info("realtime.update_score", update);
  return { accepted: true, roomId };
});

iii.registerFunction({ id: "rooms::on-score-change" }, async (event: any) => {
  const logger = new Logger();
  iii.trigger({
    function_id: "stream::send",
    payload: {
      stream_name: "room-updates",
      group_id: event.new_value.roomId,
      id: `score-${Date.now()}`,
      event_type: "room.updated",
      data: event.new_value,
    },
    action: TriggerAction.Void(),
  });
  logger.info("realtime.broadcast_update", { roomId: event.new_value.roomId });
  return { delivered: true };
});

iii.registerTrigger({
  type: "state",
  function_id: "rooms::on-score-change",
  config: { scope: "room-state" },
});

iii.registerTrigger({
  type: "http",
  function_id: "rooms::join",
  config: { api_path: "/rooms/join", http_method: "POST" },
});

iii.registerTrigger({
  type: "http",
  function_id: "rooms::update-score",
  config: { api_path: "/rooms/:roomId/score", http_method: "POST" },
});
