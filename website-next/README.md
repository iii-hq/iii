# iii.dev (Next.js)

The iii.dev website rebuilt on **Next.js 16 (App Router) + TypeScript + Tailwind CSS v4 + shadcn/ui + Motion**.
This app runs next to the current Astro site in [`../website`](../website) until it replaces it.

Status: **landing page only** (`/`). The manifesto, privacy policy, blog, and roadmap still live in the Astro site,
and nav links point to `https://iii.dev/...` for now.

## Develop

```bash
pnpm install                              # from the repo root (pnpm workspace)
pnpm --filter iii-website-next dev        # http://localhost:3100
pnpm --filter iii-website-next build
pnpm --filter iii-website-next type-check
pnpm --filter iii-website-next lint       # biome
```

## Layout

```
src/
├── app/
│   ├── layout.tsx          fonts (Chivo Mono), metadata/SEO, pre-paint theme, analytics, cookie banner
│   ├── page.tsx            the landing page: assembles the sections in order + JSON-LD
│   ├── globals.css         brand tokens → Tailwind colors, shadcn token mapping, base styles
│   └── fonts/              Chivo Mono variable fonts (next/font/local)
├── components/
│   ├── landing/            one component (or folder) per landing section
│   ├── site/               shared chrome: nav, scroll nav, footer, cookie banner, analytics, logo, icons
│   └── ui/                 shadcn/ui components
├── hooks/                  use-theme, use-copy, …
└── lib/                    site links/constants, analytics, email subscribe, theme, JSON-LD, cn()
public/
└── console-demo/           vendored console build embedded by the "same run" section (see ../website/scripts/sync-console-demo.sh)
```

## Design tokens

The brand palette from the Astro site is exposed as Tailwind colors: `bg-bg`, `bg-panel`, `bg-paper-2`, `text-ink`,
`text-ink-soft`, `text-ink-faint`, `text-ink-ghost`, `border-rule`, `border-rule-2`, and `text-brand` / `bg-brand`
(orange in light mode, blue in dark mode). Dark mode is the `.dark` class on `<html>`, set before first paint from the
`iii_theme` localStorage key (the same key the Astro site uses). Corners are square: the radius tokens are 0.

shadcn components pick up the brand automatically (`--primary` = ink, `--border` = rule, and so on). Add one with:

```bash
cd website-next && pnpm dlx shadcn@latest add <component>
```

## Environment

See [`.env.example`](./.env.example). With nothing set, analytics never load and email signups aren't sent anywhere,
which is what you want for preview deployments.

## Deploy (Vercel)

Import the repo in Vercel, set **Root Directory** to `website-next`, and keep the Next.js framework preset. Vercel
detects pnpm from the root lockfile.
