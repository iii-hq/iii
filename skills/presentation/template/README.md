# deck content layer

This is one presentation's content layer, scaffolded by the `/presentation`
skill into `<base>/<slug>/` (in iii: `website/roadmap/<slug>/`). It has
no package.json, no config, no lockfile — dependencies, build, and the shared
component library live in the base project one level up. Everything visual
imports from `@lib`; see the base's `COMPONENTS.md` before building anything
new. Dev: `pnpm dev` from the base serves this deck at `/<slug>/`.
