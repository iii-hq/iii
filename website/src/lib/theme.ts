// Theme contract shared with the Astro site: the 'iii_theme' localStorage key.
// Three preferences: "light", "dark", or "system" (follow the OS). The redesign
// is dark-first, so a visitor with nothing stored gets dark. The inline script
// below runs before first paint (see ThemeScript), so there is never a flash.
export const THEME_KEY = "iii_theme"
export const THEME_COLOR = { light: "#ffffff", dark: "#0c0c0b" } as const

/** What the visitor picked. */
export type ThemePreference = "light" | "dark" | "system"
/** What is actually on screen. */
export type Theme = "light" | "dark"

export const THEME_PREFERENCES: ThemePreference[] = ["light", "system", "dark"]

export const DARK_QUERY = "(prefers-color-scheme: dark)"

export function resolveTheme(pref: ThemePreference): Theme {
  if (pref === "system") return matchMedia(DARK_QUERY).matches ? "dark" : "light"
  return pref
}

export const themeBootScript = `(function(){try{
var r=document.documentElement;
var s=localStorage.getItem(${JSON.stringify(THEME_KEY)});
var p=(s==="light"||s==="system")?s:"dark";
var d=p==="system"?matchMedia(${JSON.stringify(DARK_QUERY)}).matches:p==="dark";
r.classList.toggle("dark",d);r.dataset.theme=d?"dark":"light";r.dataset.themePref=p;
var m=document.querySelector('meta[name="theme-color"]');if(m)m.content=d?"${THEME_COLOR.dark}":"${THEME_COLOR.light}";
}catch(e){}})();`
