import type { NextConfig } from "next"

const nextConfig: NextConfig = {
  // The whole site is prerendered into dist/ and served from S3 + CloudFront
  // (.github/workflows/deploy-website.yml). scripts/finalize-dist.ts then gives
  // the /blog and /roadmap pages the directory shape the edge function expects.
  output: "export",
  distDir: "dist",
  // No image server in a static export; the banners are already sized in public/.
  images: { unoptimized: true },
  turbopack: {
    resolveAlias: {
      // The roadmap decks read the spec list from a module generated before the
      // build (scripts/generate-roadmap-manifest.ts), the way their Vite plugin did.
      "virtual:spec-manifest": "./roadmap/generated/spec-manifest.ts",
    },
  },
}

export default nextConfig
