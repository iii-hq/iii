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
    title: "iii — Functions, Triggers, Workers. One engine, one protocol.",
    description:
      "iii turns distributed backend complexity into a simple set of real-time, interoperable primitives called Functions, Triggers, and Workers. The result is coordinated execution that behaves as if it were a single runtime.",
    indexable: true,
    ogTitle: "iii — Functions, Triggers, Workers",
  },
  {
    path: "/manifesto",
    title: "Manifesto | iii — Everything is a worker",
    description:
      "Unix made everything a file. React made everything a component. iii makes everything a worker. Three primitives, one engine, one protocol — and an integration cost that scales linearly instead of quadratically as your system grows.",
    indexable: true,
    ogTitle: "iii Manifesto — Everything is a worker",
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
