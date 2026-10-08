# new-website

The iii.dev website rebuilt on Next.js 16, Tailwind CSS v4 and [shadcn/ui](https://ui.shadcn.com) on top of [Base UI](https://base-ui.com) (`base-nova` style). The current site in `website/` stays live until this one replaces it.

## Develop

From the repo root:

```bash
pnpm install
pnpm dev:new-website        # http://localhost:3100
```

Or inside this directory: `pnpm dev`, `pnpm build`, `pnpm type-check`, `pnpm lint`.

## Adding components

Components come from the shadcn registry and are built on `@base-ui/react` primitives (not Radix):

```bash
pnpm dlx shadcn@latest add dialog
```

They land in `components/ui/` and are imported with the `@/` alias:

```tsx
import { Button } from "@/components/ui/button"
```

## Formatting

This package uses the repo's Biome config (`biome.json` at the root), not Prettier or ESLint.

## Environment

Copy `.env.example` to `.env.local`. The only variable is optional:

- `NEXT_PUBLIC_MAILMODO_FORM_URL`: Mailmodo form endpoint for the updates signup in the final CTA. Without it the form succeeds locally and does not send anywhere.

Live GitHub and Discord counts need no keys; they use public endpoints and are cached for an hour.
