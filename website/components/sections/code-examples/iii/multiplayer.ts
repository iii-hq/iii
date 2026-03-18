import { registerWorker, Logger, TriggerAction } from "iii-sdk";

const iii = registerWorker(
  process.env.III_ENGINE_URL || "ws://localhost:49134",
  {
    workerName: "multiplayer-iii",
  },
);

iii.registerFunction({ id: "rooms::join-player" }, async (request: any) => {
  const logger = new Logger();
  const roomId = request.body.roomId;
  const playerId = request.body.playerId;
  await iii.trigger({
    function_id: "state::set",
    payload: {
      scope: "room-players",
      key: `${roomId}:${playerId}`,
      value: {
        _key: `${roomId}:${playerId}`,
        roomId,
        playerId,
        score: 0,
        joinedAt: new Date().toISOString(),
      },
    },
  });
  const players = await iii.trigger({
    function_id: "state::list",
    payload: { scope: "room-players" },
  });
  const count = players.filter((p: any) => p.roomId === roomId).length;
  iii.trigger({
    function_id: "stream::send",
    payload: {
      stream_name: "room-presence",
      group_id: roomId,
      id: `join-${Date.now()}`,
      event_type: "presence.updated",
      data: { roomId, count },
    },
    action: TriggerAction.Void(),
  });
  logger.info("multiplayer.player_join", {
    roomId,
    playerId,
    count,
  });
  return { roomId, playerId, count };
});

iii.registerFunction({ id: "rooms::set-score" }, async (request: any) => {
  const logger = new Logger();
  const roomId = request.params.roomId;
  const playerId = request.body.playerId;
  const score = Number(request.body.score);
  await iii.trigger({
    function_id: "state::set",
    payload: {
      scope: "room-scores",
      key: `${roomId}:${playerId}`,
      value: {
        _key: `${roomId}:${playerId}`,
        roomId,
        playerId,
        score,
        updatedAt: new Date().toISOString(),
      },
    },
  });
  logger.info("multiplayer.score_update", {
    roomId,
    playerId,
    score,
  });
  return { accepted: true, roomId };
});

iii.registerFunction({ id: "rooms::on-score-updated" }, async (event: any) => {
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
  logger.info("multiplayer.broadcast_update", {
    roomId: event.new_value.roomId,
    playerId: event.new_value.playerId,
  });
  return { ok: true };
});

iii.registerFunction({ id: "rooms::state" }, async (request: any) => {
  const roomId = request.params.roomId;
  const players = await iii.trigger({
    function_id: "state::list",
    payload: { scope: "room-players" },
  });
  const scores = await iii.trigger({
    function_id: "state::list",
    payload: { scope: "room-scores" },
  });
  return {
    roomId,
    players: players.filter((p: any) => p.roomId === roomId).length,
    scores: scores.filter((s: any) => s.roomId === roomId),
  };
});

iii.registerTrigger({
  type: "state",
  function_id: "rooms::on-score-updated",
  config: { scope: "room-scores" },
});

iii.registerTrigger({
  type: "http",
  function_id: "rooms::join-player",
  config: {
    api_path: "/rooms/join",
    http_method: "POST",
  },
});

iii.registerTrigger({
  type: "http",
  function_id: "rooms::set-score",
  config: {
    api_path: "/rooms/:roomId/score",
    http_method: "POST",
  },
});

iii.registerTrigger({
  type: "http",
  function_id: "rooms::state",
  config: {
    api_path: "/rooms/:roomId/state",
    http_method: "GET",
  },
});
