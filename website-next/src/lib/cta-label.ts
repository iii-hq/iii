/** The old scripts' `cta_label` rule: lowercase, "↗" dropped, whitespace → "_". */
export function ctaLabel(text: string) {
  return text.replace(/↗/g, "").trim().toLowerCase().replace(/\s+/g, "_")
}
