const MIN_TEXT_LENGTH = 14;

interface MarkdownGenerationOptions {
  skipLayoutChecks?: boolean;
}

const SECTION_TITLES: Record<string, string> = {
  hero: "Overview",
  "hello-world": "Polyglot Runtime",
  architecture: "Architecture",
  observability: "Observability",
  "agent-ready": "Agent Readiness",
};

function normalizeText(value: string): string {
  return value.replace(/\s+/g, " ").trim();
}

function isHiddenElement(
  element: Element,
  options?: MarkdownGenerationOptions,
): boolean {
  const htmlElement = element as HTMLElement;
  const win = element.ownerDocument?.defaultView || window;
  const style = win.getComputedStyle(htmlElement);
  if (style.display === "none" || style.visibility === "hidden") {
    return true;
  }
  if (options?.skipLayoutChecks) {
    return false;
  }
  const rect = htmlElement.getBoundingClientRect();
  return rect.width === 0 || rect.height === 0;
}

function isInsideIgnoredNode(element: Element): boolean {
  return Boolean(
    element.closest(
      "pre, code, script, style, svg, canvas, button, [data-machine-exclude='true']",
    ),
  );
}

function getHeadingLevel(tagName: string): number {
  switch (tagName.toLowerCase()) {
    case "h1":
      return 3;
    case "h2":
      return 3;
    case "h3":
      return 4;
    default:
      return 4;
  }
}

function extractCandidateText(candidate: HTMLElement): string {
  const clone = candidate.cloneNode(true) as HTMLElement;
  clone
    .querySelectorAll("[aria-hidden='true'], [data-machine-exclude='true']")
    .forEach((node) => node.remove());
  return normalizeText(clone.textContent || "");
}

function extractSectionMarkdown(
  section: HTMLElement,
  globalSeen: Set<string>,
  options?: MarkdownGenerationOptions,
): string {
  const lines: string[] = [];
  const sectionId = section.getAttribute("data-machine-section") || "";
  const sectionTitle =
    SECTION_TITLES[sectionId] ||
    normalizeText(section.querySelector("h1, h2, h3")?.textContent || "");

  if (sectionTitle) {
    lines.push(`## ${sectionTitle}`);
  }

  const candidates = Array.from(
    section.querySelectorAll<HTMLElement>("h1, h2, h3, h4, p, li"),
  );

  for (const candidate of candidates) {
    if (
      isInsideIgnoredNode(candidate) ||
      isHiddenElement(candidate, options) ||
      candidate.closest("nav")
    ) {
      continue;
    }

    const text = extractCandidateText(candidate);
    if (
      text.length < MIN_TEXT_LENGTH ||
      globalSeen.has(text) ||
      text === sectionTitle
    ) {
      continue;
    }

    globalSeen.add(text);
    const tagName = candidate.tagName.toLowerCase();

    if (tagName === "li") {
      lines.push(`- ${text}`);
      continue;
    }

    if (tagName.startsWith("h")) {
      const headingLevel = getHeadingLevel(tagName);
      lines.push(`${"#".repeat(headingLevel)} ${text}`);
      continue;
    }

    if (tagName === "p") {
      lines.push(text);
    }
  }

  return lines.join("\n");
}

export function generateHomepageMarkdownFromDocument(
  documentRef: Document,
  options?: MarkdownGenerationOptions,
): string {
  const sections = Array.from(
    documentRef.querySelectorAll<HTMLElement>("[data-machine-section]"),
  ).filter((section) => {
    if (section.getAttribute("data-machine-exclude") === "true") {
      return false;
    }
    return section.getAttribute("data-machine-section") !== "footer";
  });

  const globalSeen = new Set<string>();
  const chunks = sections
    .map((section) => extractSectionMarkdown(section, globalSeen, options))
    .filter(Boolean);

  const intro = `# iii Homepage for AI

This markdown is auto-generated from visible homepage content and optimized for quick context.

## Code Examples
Code examples are intentionally omitted from this machine page.
For implementation details and source examples, visit:
- [iii.dev/docs](https://iii.dev/docs)
- [github.com/iii-hq/skills](https://github.com/iii-hq/skills)

## Primary Links
- [Homepage](https://iii.dev)
- [Documentation](https://iii.dev/docs)
- [GitHub](https://github.com/iii-hq/iii)
- [Skills Repository](https://github.com/iii-hq/skills)`;

  return [intro, ...chunks].join("\n\n");
}
