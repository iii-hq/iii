import { themeBootScript } from "@/lib/theme"

/** Applies the stored/system theme to <html> before first paint. Render in <head>. */
export function ThemeScript() {
  // biome-ignore lint/security/noDangerouslySetInnerHtml: static first-party boot script
  return <script dangerouslySetInnerHTML={{ __html: themeBootScript }} />
}
