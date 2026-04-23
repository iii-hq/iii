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

function injectSeo({ template, title, description, canonical, ogTitle, indexable }: InjectArgs): string {
  let out = template;

  out = out.replace(/<title>[^<]*<\/title>/, `<title>${title}</title>`);

  out = out.replace(
    /<meta\s+name="description"\s+content="[^"]*"\s*\/?>/,
    `<meta name="description" content="${description}" />`,
  );

  out = out.replace(
    /<meta\s+property="og:url"\s+content="[^"]*"\s*\/?>/,
    `<meta property="og:url" content="${canonical}" />`,
  );
  out = out.replace(
    /<meta\s+property="og:title"\s+content="[^"]*"\s*\/?>/,
    `<meta property="og:title" content="${ogTitle}" />`,
  );
  out = out.replace(
    /<meta\s+property="og:description"\s+content="[^"]*"\s*\/?>/,
    `<meta property="og:description" content="${description}" />`,
  );
  out = out.replace(
    /<meta\s+name="twitter:url"\s+content="[^"]*"\s*\/?>/,
    `<meta name="twitter:url" content="${canonical}" />`,
  );
  out = out.replace(
    /<meta\s+name="twitter:title"\s+content="[^"]*"\s*\/?>/,
    `<meta name="twitter:title" content="${ogTitle}" />`,
  );
  out = out.replace(
    /<meta\s+name="twitter:description"\s+content="[^"]*"\s*\/?>/,
    `<meta name="twitter:description" content="${description}" />`,
  );

  const robots = indexable ? "index,follow" : "noindex,follow";
  out = out.replace(
    /<link\s+rel="canonical"\s+href="[^"]*"\s*\/?>/,
    `<link rel="canonical" href="${canonical}" />`,
  );
  out = out.replace(
    /<meta\s+name="robots"\s+content="[^"]*"\s*\/?>/,
    `<meta name="robots" content="${robots}" />`,
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
