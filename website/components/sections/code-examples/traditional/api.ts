const express = require("express");
const { z } = require("zod");

const app = express();
app.use(express.json());

type Post = {
  title: string;
  body: string;
};

const posts: Post[] = [
  {
    title: "Hello API",
    body: "API handlers often keep HTTP concerns close to app logic.",
  },
];

const createPost = z.object({
  title: z.string().min(1),
  body: z.string().min(1),
});

function listPosts(_req: any, res: any) {
  res.json(posts);
}

function createPostHandler(req: any, res: any) {
  const token = String(req.header("authorization") || "").replace("Bearer ", "");

  if (token !== process.env.API_TOKEN) {
    return res.status(401).json({ error: "Unauthorized" });
  }

  const data = createPost.parse(req.body);
  const post: Post = { title: data.title, body: data.body };

  posts.unshift(post);
  res.status(201).json(post);
}

app.get("/posts", listPosts);
app.post("/posts", createPostHandler);

app.listen(3000);
