import { registerWorker, Logger, TriggerAction } from "iii-sdk";

const iii = registerWorker(
  process.env.III_ENGINE_URL || "ws://localhost:49134",
  {
    workerName: "realtime-iii",
  },
);

iii.registerFunction({ id: "stream::publish-room-score" }, async (request: any) => {
  const logger = new Logger();
  const score = {
    roomId: request.params.roomId,
    playerId: request.body.playerId,
    score: Number(request.body.score),
    at: new Date().toISOString(),
  };
  iii.trigger({
    function_id: "stream::send",
    payload: {
      stream_name: "room-updates",
      group_id: score.roomId,
      id: `score-${Date.now()}`,
      event_type: "room.score.updated",
      data: score,
    },
    action: TriggerAction.Void(),
  });
  logger.info("stream.publish_room_score", score);
  return { accepted: true, roomId: score.roomId };
});

iii.registerFunction({ id: "stream::consume-room-score" }, async (event: any) => {
  const logger = new Logger();
  const score = event.data ?? event.payload?.data ?? event;
  logger.info("stream.consume_room_score", {
    roomId: score.roomId,
    playerId: score.playerId,
  });
  return { processed: true };
});

iii.registerTrigger({
  type: "stream",
  function_id: "stream::consume-room-score",
  config: { stream_name: "room-updates" },
});

iii.registerTrigger({
  type: "http",
  function_id: "stream::publish-room-score",
  config: {
    api_path: "/rooms/:roomId/score",
    http_method: "POST",
  },
});
