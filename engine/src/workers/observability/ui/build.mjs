import { mkdir, rm } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { build } from "esbuild";

const here = dirname(fileURLToPath(import.meta.url));
const outputDirectory = resolve(here, "dist");
const production = process.env.NODE_ENV === "production" || process.env.BUILD_MODE === "release";

await rm(outputDirectory, { recursive: true, force: true });
await mkdir(outputDirectory, { recursive: true });

await build({
  entryPoints: [join(here, "page.tsx"), join(here, "styles.css")],
  outdir: outputDirectory,
  bundle: true,
  format: "esm",
  platform: "browser",
  target: ["es2020"],
  jsx: "automatic",
  minify: production,
  sourcemap: false,
  legalComments: "none",
  external: ["react", "react-dom", "react-dom/client", "react/jsx-runtime"],
  loader: { ".css": "css", ".tsx": "tsx" },
  logLevel: "info",
});

console.log("Built iii-observability UI: page.js, styles.css");
