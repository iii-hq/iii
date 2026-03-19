const MIN_TEXT_LENGTH = 14;

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

function isHiddenElement(element: Element): boolean {
  const htmlElement = element as HTMLElement;
  const style = window.getComputedStyle(htmlElement);
  if (style.display === "none" || style.visibility === "hidden") {
    return true;
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

function extractSectionMarkdown(
  section: HTMLElement,
  globalSeen: Set<string>,
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
      isHiddenElement(candidate) ||
      candidate.closest("nav")
    ) {
      continue;
    }

    const text = normalizeText(candidate.textContent || "");
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

export function generateHomepageMarkdownFromDocument(documentRef: Document): string {
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
    .map((section) => extractSectionMarkdown(section, globalSeen))
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
