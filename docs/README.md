# iii Docs

The iii documentation source lives in this directory and is served locally with Mintlify.

## Development

From the repository root:

```bash
pnpm dev:docs
```

Or directly from this directory:

```bash
npx mint dev
```

The docs config lives in `docs.json`. Local preview is typically available at `http://localhost:3000`.

## API Reference

To refresh generated SDK API reference files before previewing docs:

```bash
pnpm generate:api-docs
```
