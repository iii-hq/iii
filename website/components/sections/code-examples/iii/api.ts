const { registerWorker } = require("iii-sdk");
const { z } = require("zod");

const iii = registerWorker("ws://localhost:49134", { workerName: "blog-api" });

type Post = {
  title: string;
  body: string;
};

const posts: Post[] = [
  {
    title: "Hello API",
    body: "HTTP trigger wiring stays separate from function logic.",
  },
];

const createPost = z.object({
  title: z.string().min(1),
  body: z.string().min(1),
});

iii.registerFunction({ id: "blog::list-posts" }, async () => {
  return posts;
});

iii.registerFunction({ id: "blog::create-post" }, async (request: any) => {
  const token = String(request.headers.authorization || "").replace("Bearer ", "");
  if (token !== process.env.API_TOKEN) {
    throw new Error("Unauthorized");
  }

  const { title, body } = createPost.parse(request.body);
  const post = { title, body };

  posts.unshift(post);
  return post;
});

iii.registerTrigger({
  type: "http",
  function_id: "blog::list-posts",
  config: { api_path: "/posts", http_method: "GET" },
});
iii.registerTrigger({
  type: "http",
  function_id: "blog::create-post",
  config: { api_path: "/posts", http_method: "POST" },
});
