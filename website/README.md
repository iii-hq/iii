# iii Website

Marketing website for [iii](https://iii.dev) — the Interoperable Invocation Interface.

## Tech Stack

- React 19 + TypeScript
- Vite 6
- Tailwind CSS 4
- Motion (animations)
- Prism React Renderer (code highlighting)

## Development

```bash
npm install
npm run dev
npm run build
```

## Deployment

Deployed to [Vercel](https://vercel.com) under the **motia** org. Pushes to `main` auto-deploy to production at [iii.dev](https://iii.dev).

## Project Structure

```
├── App.tsx                  # Main app — routes, theme, layout
├── index.tsx                # React entry point
├── index.html               # HTML template + meta tags + Tailwind config
├── globals.css              # Brand colors, fonts, animations
├── types.ts                 # Shared TypeScript types
├── vite.config.ts           # Vite configuration
├── vercel.json              # Vercel rewrites (docs proxy, SPA fallback)
├── pages/
│   └── ManifestoPage.tsx    # /manifesto page
├── components/
│   ├── Logo.tsx             # iii logo
│   ├── Navbar.tsx           # Navigation bar
│   ├── Terminal.tsx          # Debug terminal (Easter egg)
│   ├── MachineView.tsx      # Machine-readable markdown (/ai)
│   ├── ModeToggle.tsx       # Human/Machine mode toggle
│   ├── LicenseModal.tsx     # License info modal
│   ├── DependencyStack.tsx  # Dependency visualization
│   ├── icons/               # Custom SVG icon components
│   ├── ui/                  # Shared UI primitives
│   └── sections/
│       ├── HeroSection.tsx           # Hero + tagline
│       ├── HelloWorldSection.tsx     # Polyglot code demo (Python/Rust/Node)
│       ├── EngineSection.tsx         # Three primitives: Function, Trigger, Worker
│       ├── CodeExamples.tsx          # 14 side-by-side comparisons (traditional vs iii)
│       ├── ExampleCodeSection.tsx    # Code examples renderer
│       ├── AgentReadySection.tsx     # 8 agent capability patterns
│       ├── DependencyVisualization.tsx
│       ├── TestimonialsSection.tsx
│       ├── WhatsCatchSection.tsx
│       └── FooterSection.tsx
└── public/
    ├── favicon.svg
    ├── og-image.png          # OG preview (1200x630)
    ├── og-banner.png         # Banner variant (1200x630)
    ├── og-image-square.png   # Square variant (1200x1200)
    ├── og-image.svg          # SVG fallback
    ├── brand/                # Brand assets (logos, patterns)
    └── fonts/                # ChivoMono font family
```

## License

Apache 2.0
