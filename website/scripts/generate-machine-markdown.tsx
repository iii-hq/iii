import fs from "node:fs/promises";
import path from "node:path";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { JSDOM } from "jsdom";
import { HeroSection } from "../components/sections/HeroSection";
import { WhyIIISection } from "../components/sections/WhyIIISection";
import { EngineSection } from "../components/sections/EngineSection";
import { HelloWorldSection } from "../components/sections/HelloWorldSection";
import { ObservabilitySection } from "../components/sections/ObservabilitySection";
import { generateHomepageMarkdownFromDocument } from "../lib/machineMarkdown";

function HomepageSectionsSnapshot() {
  return (
    <main>
      <div data-machine-section="hero">
        <HeroSection isDarkMode />
      </div>
      <div data-machine-section="why-iii">
        <WhyIIISection isDarkMode />
      </div>
      <div data-machine-section="architecture">
        <EngineSection isDarkMode />
      </div>
      <div data-machine-section="hello-world">
        <HelloWorldSection isDarkMode />
      </div>
      <div data-machine-section="observability">
        <ObservabilitySection isDarkMode />
      </div>
      <div data-machine-section="footer" />
    </main>
  );
}

async function generateMachineMarkdownFile() {
  const markup = renderToStaticMarkup(<HomepageSectionsSnapshot />);
  const dom = new JSDOM(`<!doctype html><html><body>${markup}</body></html>`);
  const markdown = generateHomepageMarkdownFromDocument(dom.window.document, {
    skipLayoutChecks: true,
  });

  const outputPath = path.resolve(process.cwd(), "public/ai.md");
  await fs.mkdir(path.dirname(outputPath), { recursive: true });
  await fs.writeFile(outputPath, `${markdown.trim()}\n`, "utf8");

  const htmlOutputPath = path.resolve(process.cwd(), "public/ai/index.html");
  await fs.mkdir(path.dirname(htmlOutputPath), { recursive: true });
  await fs.writeFile(
    htmlOutputPath,
    buildAiHtmlDocument(markdown.trim()),
    "utf8",
  );

  console.log(
    `Generated ${path.relative(process.cwd(), outputPath)} and ${path.relative(process.cwd(), htmlOutputPath)}`,
  );
}

function escapeHtml(value: string): string {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;");
}

function buildAiHtmlDocument(markdown: string): string {
  const escaped = escapeHtml(markdown);
  return `<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>iii Homepage for AI</title>
    <meta name="robots" content="index,follow" />
    <style>
      :root {
        color-scheme: dark;
      }
      body {
        margin: 0;
        background: #000;
        color: #f3f4f6;
        font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace;
      }
      main {
        max-width: 900px;
        margin: 0 auto;
        padding: 32px 20px;
      }
      pre {
        margin: 0;
        white-space: pre-wrap;
        word-break: break-word;
        line-height: 1.5;
        font-size: 14px;
      }
    </style>
  </head>
  <body>
    <main>
      <pre id="machine-markdown">${escaped}</pre>
    </main>
  </body>
</html>
`;
}

generateMachineMarkdownFile().catch((error) => {
  console.error("Failed generating machine markdown:", error);
  process.exitCode = 1;
});
