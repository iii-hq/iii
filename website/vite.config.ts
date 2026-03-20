import fs from "node:fs";
import path from "path";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [
    react(),
    {
      name: "serve-static-ai-page",
      configureServer(server) {
        server.middlewares.use((req, res, next) => {
          const requestUrl = req.url || "";
          if (
            requestUrl === "/ai" ||
            requestUrl === "/ai/" ||
            requestUrl.startsWith("/ai?")
          ) {
            const aiHtmlPath = path.resolve(__dirname, "public/ai/index.html");
            if (fs.existsSync(aiHtmlPath)) {
              res.statusCode = 200;
              res.setHeader("Content-Type", "text/html; charset=utf-8");
              res.end(fs.readFileSync(aiHtmlPath, "utf8"));
              return;
            }
          }
          next();
        });
      },
    },
  ],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "."),
    },
  },
  appType: "spa",
});
