import fs from "node:fs/promises";
import path from "node:path";
import { ROUTES, SITE_ORIGIN } from "./routes";

const TEMPLATE_PATH = path.resolve(process.cwd(), "index.html");
const DIST_DIR = path.resolve(process.cwd(), "dist");

interface InjectArgs {
  template: string;
  title: string;
  description: string;
  canonical: string;
  ogTitle: string;
  indexable: boolean;
}

function escapeHtml(value: string): string {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}

function replaceOrThrow(source: string, pattern: RegExp, replacement: string, label: string): string {
  if (!pattern.test(source)) {
    throw new Error(
      `prerender: pattern for "${label}" did not match index.html. Template format may have shifted. Regex: ${pattern}`,
    );
  }
  return source.replace(pattern, replacement);
}

function injectSeo({ template, title, description, canonical, ogTitle, indexable }: InjectArgs): string {
  const safeTitle = escapeHtml(title);
  const safeDescription = escapeHtml(description);
  const safeCanonical = escapeHtml(canonical);
  const safeOgTitle = escapeHtml(ogTitle);
  const robots = indexable ? "index,follow" : "noindex,follow";

  let out = template;
  out = replaceOrThrow(out, /<title>[^<]*<\/title>/, `<title>${safeTitle}</title>`, "title");
  out = replaceOrThrow(
    out,
    /<meta\s+name="description"\s+content="[^"]*"\s*\/?>/,
    `<meta name="description" content="${safeDescription}" />`,
    "meta description",
  );
  out = replaceOrThrow(
    out,
    /<meta\s+property="og:url"\s+content="[^"]*"\s*\/?>/,
    `<meta property="og:url" content="${safeCanonical}" />`,
    "og:url",
  );
  out = replaceOrThrow(
    out,
    /<meta\s+property="og:title"\s+content="[^"]*"\s*\/?>/,
    `<meta property="og:title" content="${safeOgTitle}" />`,
    "og:title",
  );
  out = replaceOrThrow(
    out,
    /<meta\s+property="og:description"\s+content="[^"]*"\s*\/?>/,
    `<meta property="og:description" content="${safeDescription}" />`,
    "og:description",
  );
  out = replaceOrThrow(
    out,
    /<meta\s+name="twitter:url"\s+content="[^"]*"\s*\/?>/,
    `<meta name="twitter:url" content="${safeCanonical}" />`,
    "twitter:url",
  );
  out = replaceOrThrow(
    out,
    /<meta\s+name="twitter:title"\s+content="[^"]*"\s*\/?>/,
    `<meta name="twitter:title" content="${safeOgTitle}" />`,
    "twitter:title",
  );
  out = replaceOrThrow(
    out,
    /<meta\s+name="twitter:description"\s+content="[^"]*"\s*\/?>/,
    `<meta name="twitter:description" content="${safeDescription}" />`,
    "twitter:description",
  );
  out = replaceOrThrow(
    out,
    /<link\s+rel="canonical"\s+href="[^"]*"\s*\/?>/,
    `<link rel="canonical" href="${safeCanonical}" />`,
    "canonical",
  );
  out = replaceOrThrow(
    out,
    /<meta\s+name="robots"\s+content="[^"]*"\s*\/?>/,
    `<meta name="robots" content="${robots}" />`,
    "robots",
  );

  const lastmod = new Date().toISOString().slice(0, 10);
  out = out.replace(
    "</head>",
    `    <meta name="article:modified_time" content="${lastmod}" />\n    <meta property="og:updated_time" content="${lastmod}" />\n  </head>`,
  );

  return out;
}

async function prerender() {
  const template = await fs.readFile(TEMPLATE_PATH, "utf8");

  for (const route of ROUTES) {
    if (route.path === "/ai") continue;

    const canonical = `${SITE_ORIGIN}${route.path === "/" ? "/" : route.path}`;
    const html = injectSeo({
      template,
      title: route.title,
      description: route.description,
      canonical,
      ogTitle: route.ogTitle ?? route.title,
      indexable: route.indexable,
    });

    const outPath =
      route.path === "/"
        ? path.join(DIST_DIR, "index.html")
        : path.join(DIST_DIR, route.path.replace(/^\//, ""), "index.html");

    await fs.mkdir(path.dirname(outPath), { recursive: true });
    await fs.writeFile(outPath, html, "utf8");
    console.log(`prerendered ${route.path} -> ${path.relative(process.cwd(), outPath)}`);
  }
}

prerender().catch((error) => {
  console.error("prerender failed:", error);
  process.exitCode = 1;
});
