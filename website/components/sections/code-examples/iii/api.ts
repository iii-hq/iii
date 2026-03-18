import { registerWorker, Logger } from "iii-sdk";
import { z } from "zod";

const iii = registerWorker(
  process.env.III_ENGINE_URL || "ws://localhost:49134",
  {
    workerName: "blog-api",
  },
);

type Post = {
  title: string;
  body: string;
};

const posts: Post[] = [];

const createPost = z.object({
  title: z.string().min(1),
  body: z.string().min(1),
});

iii.registerFunction({ id: "blog::list-posts" }, async () => {
  const logger = new Logger();
  // ...read model query...
  logger.info("api.list_posts", { count: posts.length });
  return { posts };
});

iii.registerFunction({ id: "blog::create-post" }, async (request: any) => {
  const logger = new Logger();
  const { title, body } = createPost.parse(request.body);
  const post = { title, body };
  // ...domain rules and side effects...
  posts.unshift(post);
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
