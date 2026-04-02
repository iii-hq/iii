# Design System — iii Console

## Product Context
- **What this is:** Developer console for the iii backend execution engine — inspect functions, triggers, states, streams, queues, traces, logs, and config
- **Who it's for:** Backend developers managing iii-powered services
- **Space/industry:** Developer infrastructure (peers: Vercel, Railway, Grafana, Datadog, Linear, Neon)
- **Project type:** Data-heavy dashboard / developer tool

## Aesthetic Direction
- **Direction:** Industrial/Utilitarian
- **Decoration level:** Minimal — typography and color do all the work. No gradients, no glow effects, no atmospheric backgrounds
- **Mood:** An engine control room, not a marketing site. Every element earns its place. Raw, functional, serious
- **Reference sites:** Vercel (clean geometry), Linear (dark + restrained), Resend (editorial confidence), Neon (dev-focused dark)

## Typography
- **Display/Hero:** Geist Sans 700 — clean geometric sans that feels native to the dev tool space, tight tracking (-0.02em) at large sizes
- **Body/UI:** Geist Sans 400-500 — readable at small sizes for sidebar nav, descriptions, metadata, labels
- **UI/Labels:** Geist Sans 600 — uppercase with 0.04em tracking for column headers and section labels
- **Data/Tables:** JetBrains Mono 400-500 — tabular-nums enabled, used for function names, IDs, timestamps, metrics, log lines, and all developer-facing data
- **Code:** JetBrains Mono 400
- **Loading:** Google Fonts CDN — `https://fonts.googleapis.com/css2?family=Geist:wght@400;500;600;700&family=JetBrains+Mono:wght@400;500;600;700&display=swap`
- **Scale:**
  - Page heading: 18px (text-lg) / Geist 600 / tight tracking — compact for dev console
  - Section title: 24px / Geist 700 / -0.01em
  - Card heading: 18px / Geist 600
  - Body: 14px / Geist 400
  - Small body: 13px / Geist 400
  - Label: 12px / Geist 600 / uppercase / 0.04em
  - Mono display: 20px / JBM 600 / tabular-nums
  - Mono body: 13px / JBM 400
  - Mono small: 11px / JBM 400
- **Font assignment rule:** Geist Sans handles "interface" (navigation, headings, descriptions, button labels, column headers). JetBrains Mono handles "data" (function names, IDs, timestamps, metrics, JSON, log lines, terminal output, input values, code)

## Color
- **Approach:** Restrained — yellow is rare and meaningful. Most of the UI is grayscale. When color appears, it means something
- **Primary/Accent:** #F3F724 — electric yellow. Nobody else in the dev tool space uses this. It says "engine" not "cloud service." Use sparingly: CTAs, active states, the logo, section labels
- **Background:** #000000
- **Surface/Sidebar:** #0A0A0A
- **Elevated:** #111111
- **Hover:** #1A1A1A
- **Border:** #2D2D2D
- **Border subtle:** #1D1D1D
- **Neutrals:** #1D1D1D (dark gray) → #5B5B5B (muted) → #9CA3AF (secondary text) → #F4F4F4 (foreground text)
- **Semantic:** success #22C55E, warning #F3F724, error #EF4444, info #3B82F6
- **Semantic subtle (backgrounds):** success rgba(34,197,94,0.1), error rgba(239,68,68,0.1), warning rgba(243,247,36,0.08), info rgba(59,130,246,0.1)
- **Accent subtle:** rgba(243,247,36,0.08) — for active nav item backgrounds, hover highlights
- **Accent text (on accent bg):** #000000
- **Dark mode:** This is the default and primary theme
- **Light mode strategy:** Background #FAFAFA, surfaces #FFFFFF, accent reduced to #C8CC00 (darkened yellow for contrast), semantic colors darkened ~10%, borders #E0E0E0

## Spacing
- **Base unit:** 4px
- **Density:** Comfortable
- **Scale:** 2xs(2px) xs(4px) sm(8px) md(16px) lg(24px) xl(32px) 2xl(48px) 3xl(64px)

## Layout
- **Approach:** Grid-disciplined — strict alignment for data-heavy pages (functions, triggers, logs, traces). The flow editor is the exception where creative layout applies
- **Sidebar:** Fixed 224px (desktop), collapsible on mobile
- **Max content width:** 1100px for settings/config pages, full-width for data tables
- **Border radius:** sm:4px, md:6px, lg:10px, full:9999px
  - Cards/panels: lg (10px)
  - Buttons/inputs: md (6px)
  - Status badges/pills: full (9999px)
  - Small elements (log line hover): 2px

## Motion
- **Approach:** Minimal-functional — only transitions that aid comprehension
- **Easing:** enter(ease-out / cubic-bezier(0.16,1,0.3,1)) exit(ease-in) move(ease-in-out)
- **Duration:** micro(80ms) short(150ms) medium(300ms) long(500ms)
- **Usage:** Panel slides, state transitions on health indicators, tab switches, hover feedback. No decorative animation

## CSS Custom Properties

```css
:root {
  --font-sans: 'Geist', -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
  --font-mono: 'JetBrains Mono', 'Fira Code', 'Cascadia Code', monospace;

  --background: #000000;
  --foreground: #F4F4F4;
  --sidebar: #0A0A0A;
  --elevated: #111111;
  --hover: #1A1A1A;
  --border: #2D2D2D;
  --border-subtle: #1D1D1D;
  --muted: #5B5B5B;
  --secondary: #9CA3AF;
  --accent: #F3F724;
  --accent-hover: #D4D820;
  --accent-subtle: rgba(243, 247, 36, 0.08);
  --accent-text: #000000;

  --success: #22C55E;
  --warning: #F3F724;
  --error: #EF4444;
  --info: #3B82F6;
}
```

## Decisions Log
| Date | Decision | Rationale |
|------|----------|-----------|
| 2026-03-20 | Initial design system created | Created by /design-consultation based on competitive research of Vercel, Railway, Linear, Neon, Resend, Supabase, Grafana |
| 2026-03-20 | Hybrid typography: Geist Sans + JetBrains Mono | Sans-serif for interface readability, monospace for data integrity. Unusual in the category — most tools pick one or the other. Gives iii a "terminal that grew a proper UI" feel |
| 2026-03-20 | Electric yellow accent #F3F724 | Unique in the dev tool space where blue/green/purple dominate. Industrial, aggressive, memorable. Use sparingly to maintain impact |
| 2026-03-20 | Zero decoration policy | No gradients, glow effects, or atmospheric backgrounds. Loads faster, ages better, feels more serious than competitors using visual effects |
| 2026-03-20 | Info color changed from #5B5B5B to #3B82F6 | Previous info color was indistinguishable from muted text. Blue provides clear semantic meaning for informational alerts |
