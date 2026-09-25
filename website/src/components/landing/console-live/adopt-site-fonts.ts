/**
 * The vendored console demo was built with its own type (Chivo Mono for UI,
 * Geist for prose, Geist Mono for code). The site serves exactly two families
 * through next/font, Inter and Geist Mono, and the demo ships no font files of
 * its own. Once the same-origin frame loads we copy both families' @font-face
 * rules into it and rewrite its declarations: Chivo Mono → Geist Mono, Geist →
 * Inter. The frame then loads the same self-hosted files the page already has.
 */
export function adoptSiteFonts(frame: Document) {
  const site = getComputedStyle(document.documentElement)
  const inter = site.getPropertyValue("--font-inter").trim()
  const geistMono = site.getPropertyValue("--font-geist-mono").trim()
  if (!inter || !geistMono) return

  const sans = `${inter}, ui-sans-serif, system-ui, sans-serif`
  const mono = `${geistMono}, ui-monospace, SFMono-Regular, Menlo, monospace`
  const families = [inter, geistMono]

  // 1) Bring both families' @font-face rules into the frame. Their url()s are
  //    relative to the stylesheet they came from, so make them absolute before
  //    the frame (which lives in another folder) resolves them.
  const faces: string[] = []
  for (const sheet of Array.from(document.styleSheets)) {
    let rules: CSSRuleList
    try {
      rules = sheet.cssRules
    } catch {
      continue
    }
    const base = sheet.href ?? location.href
    for (const rule of Array.from(rules)) {
      if (!(rule instanceof CSSFontFaceRule)) continue
      const face = rule.style.getPropertyValue("font-family").replace(/['"]/g, "")
      if (!families.some((f) => f.includes(face))) continue
      faces.push(
        rule.cssText.replace(
          /url\((['"]?)([^'")]+)\1\)/g,
          (_, q: string, u: string) => `url(${q}${new URL(u, base).href}${q})`,
        ),
      )
    }
  }
  const style = frame.createElement("style")
  style.setAttribute("data-site-fonts", "")
  style.textContent = `${faces.join("\n")}\n:root{--font-sans:${sans};--font-mono:${mono};--font-geist-mono:${mono}}`
  frame.head.appendChild(style)

  // 2) Rewrite direct font-family declarations in the demo's own rules.
  for (const sheet of Array.from(frame.styleSheets)) {
    if (sheet.ownerNode === style) continue
    let rules: CSSRuleList
    try {
      rules = sheet.cssRules
    } catch {
      continue
    }
    for (const rule of Array.from(rules)) {
      if (!(rule instanceof CSSStyleRule)) continue
      const family = rule.style.fontFamily
      if (!family) continue
      const priority = rule.style.getPropertyPriority("font-family")
      if (/chivo|geist mono/i.test(family)) rule.style.setProperty("font-family", mono, priority)
      else if (/\bgeist\b/i.test(family)) rule.style.setProperty("font-family", sans, priority)
    }
  }
}
