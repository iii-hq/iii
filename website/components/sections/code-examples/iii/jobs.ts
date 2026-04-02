import { registerWorker, Logger, TriggerAction } from "iii-sdk";

const iii = registerWorker(
  process.env.III_ENGINE_URL || "ws://localhost:49134",
  {
    workerName: "jobs-iii",
  },
);

iii.registerFunction(
  "video::enqueue-transcode",
  async (request: any) => {
    const logger = new Logger();
    const jobId = request.body.jobId ?? `job-${Date.now()}`;
    const payload = {
      jobId,
      assetId: request.body.assetId,
      profile: request.body.profile ?? "1080p",
    };
    await iii.trigger({
      function_id: "state::set",
      payload: {
        scope: "jobs",
        key: jobId,
        value: {
          _key: jobId,
          ...payload,
          status: "queued",
        },
      },
    });
    iii.trigger({
      function_id: "video::transcode",
      payload,
      action: TriggerAction.Enqueue({
        queue: "video-transcode",
      }),
    });
    logger.info("jobs.enqueue_transcode.queued", {
      jobId,
      assetId: request.body.assetId,
    });
    return { jobId, queue: "video-transcode" };
  },
);

iii.registerFunction("video::transcode", async (data: any) => {
  const logger = new Logger();
  logger.info("jobs.video_transcode.started", {
    jobId: data.jobId,
    assetId: data.assetId,
  });
  const result = await iii.trigger({
    function_id: "media-worker::transcode",
    payload: data,
  });
  await iii.trigger({
    function_id: "state::set",
    payload: {
      scope: "jobs",
      key: data.jobId,
      value: {
        _key: data.jobId,
        ...result,
      },
    },
  });
  logger.info("jobs.video_transcode.completed", {
    jobId: data.jobId,
    output: result.output,
  });
  return result;
});

iii.registerFunction("video::job-status", async (request: any) => {
  const logger = new Logger();
  let job = await iii.trigger({
    function_id: "state::get",
    payload: {
      scope: "jobs",
      key: request.params.jobId,
    },
  });
  if (!job) {
    logger.warn("jobs.lookup.not_found", {
      jobId: request.params.jobId,
    });
    const error = new Error("Job not found") as Error & {
      status: number;
    };
    error.status = 404;
    throw error;
  }
  logger.info("jobs.lookup.found", {
    jobId: request.params.jobId,
    state: job.status,
  });
  return {
    jobId: job.jobId,
    state: job.status,
    result: job,
  };
});

iii.registerTrigger({
  type: "http",
  function_id: "video::enqueue-transcode",
  config: {
    api_path: "/jobs/transcode",
    http_method: "POST",
  },
});

iii.registerTrigger({
  type: "http",
  function_id: "video::job-status",
  config: {
    api_path: "/jobs/:jobId",
    http_method: "GET",
  },
});
