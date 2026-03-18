import { registerWorker, Logger, TriggerAction } from "iii-sdk";

const iii = registerWorker(
  process.env.III_ENGINE_URL || "ws://localhost:49134",
  {
    workerName: "jobs-iii",
  },
);

iii.registerFunction(
  { id: "video::enqueue-transcode" },
  async (request: any) => {
    const logger = new Logger();
    const prepared = await iii.trigger({
      function_id: "media-service::prepare-transcode",
      payload: {
        assetId: request.body.assetId,
        profile: request.body.profile ?? "1080p",
      },
    });
    const jobId = prepared.jobId ?? `job-${Date.now()}`;
    await iii.trigger({
      function_id: "state::set",
      payload: {
        scope: "jobs",
        key: jobId,
        value: {
          _key: jobId,
          jobId,
          assetId: prepared.assetId,
          profile: prepared.profile,
          status: "queued",
        },
      },
    });
    iii.trigger({
      function_id: "video::transcode",
      payload: {
        jobId,
        assetId: prepared.assetId,
        profile: prepared.profile,
      },
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

iii.registerFunction({ id: "video::transcode" }, async (data: any) => {
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

iii.registerFunction({ id: "video::job-status" }, async (request: any) => {
  const logger = new Logger();
  let job = await iii.trigger({
    function_id: "state::get",
    payload: {
      scope: "jobs",
      key: request.params.jobId,
    },
  });
  if (!job) {
    job = await iii.trigger({
      function_id: "jobs-service::lookup",
      payload: {
        jobId: request.params.jobId,
      },
    });
  }
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
