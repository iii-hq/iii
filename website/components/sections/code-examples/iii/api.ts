import { registerWorker, Logger, TriggerAction } from "iii-sdk";

const iii = registerWorker(
  process.env.III_ENGINE_URL || "ws://localhost:49134",
  {
    workerName: "blog-api",
  },
);

iii.registerFunction({ id: "blog::list-posts" }, async () => {
  const logger = new Logger();
  const posts =
    (await iii.trigger({
      function_id: "blog-service::list-posts",
      payload: {
        limit: 50,
      },
    })) ?? [];
  logger.info("api.list_posts", { count: posts.length });
  return { posts };
});

iii.registerFunction({ id: "blog::create-post" }, async (request: any) => {
  const logger = new Logger();
  const draft = {
    title: String(request.body.title ?? "").trim(),
    body: String(request.body.body ?? "").trim(),
  };
  if (!draft.title || !draft.body) {
    const error = new Error("Title and body are required") as Error & {
      status: number;
    };
    error.status = 400;
    throw error;
  }
  const reviewed = await iii.trigger({
    function_id: "moderation-service::review-post",
    payload: draft,
  });
  if (!reviewed.approved) {
    const error = new Error("Post rejected by moderation policy") as Error & {
      status: number;
    };
    error.status = 422;
    throw error;
  }
  const post = await iii.trigger({
    function_id: "blog-service::create-post",
    payload: {
      title: reviewed.title,
      body: reviewed.body,
      authorId: request.body.authorId ?? "anonymous",
    },
  });
  iii.trigger({
    function_id: "publish",
    payload: {
      topic: "blog.post.created",
      data: {
        postId: post.id,
        title: post.title,
      },
    },
    action: TriggerAction.Void(),
  });
  logger.info("api.create_post.created", { title: post.title });
  return { post };
});

iii.registerTrigger({
  type: "http",
  function_id: "blog::list-posts",
  config: {
    api_path: "/posts",
    http_method: "GET",
  },
});

iii.registerTrigger({
  type: "http",
  function_id: "blog::create-post",
  config: {
    api_path: "/posts",
    http_method: "POST",
  },
});
