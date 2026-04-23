export interface RouteMeta {
  path: string;
  title: string;
  description: string;
  indexable: boolean;
  ogTitle?: string;
}

export const ROUTES: RouteMeta[] = [
  {
    path: "/",
    title: "iii — one engine, three primitives for backend execution",
    description:
      "iii replaces the six-tool backend (API framework, queue, cron, pub/sub, state store, observability) with one engine and three primitives: Function, Trigger, Worker. A single model for backend execution.",
    indexable: true,
    ogTitle: "iii — one engine, three primitives",
  },
  {
    path: "/manifesto",
    title: "Manifesto | iii",
    description:
      "Why iii exists: polyglot by design, small primitive surface area, live discovery, live extensibility, live observability. How distributed backends should work.",
    indexable: true,
    ogTitle: "iii Manifesto",
  },
  {
    path: "/ai",
    title: "iii Homepage for AI",
    description:
      "Machine-readable snapshot of the iii homepage. Plain-text markdown for LLM ingestion.",
    indexable: true,
    ogTitle: "iii | AI-readable Homepage",
  },
  {
    path: "/preview",
    title: "Sections Preview | iii (internal)",
    description: "Internal preview of homepage sections.",
    indexable: false,
  },
];

export const INDEXABLE_ROUTES = ROUTES.filter((r) => r.indexable);

export const SITE_ORIGIN = "https://iii.dev";
