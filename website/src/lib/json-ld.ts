import { links, site } from "@/lib/site"

// Structured data for the landing page (Organization, WebSite, SoftwareApplication).
export const landingJsonLd = [
  {
    "@context": "https://schema.org",
    "@type": "Organization",
    name: "iii",
    legalName: "III, Inc.",
    url: `${site.url}/`,
    logo: `${site.url}/favicon.svg`,
    description:
      "iii turns distributed backend complexity into a simple set of real-time, interoperable primitives called Functions, Triggers, and Workers. Any process that speaks the open iii protocol joins as a worker and becomes discoverable and callable across the system.",
    sameAs: [links.github],
  },
  {
    "@context": "https://schema.org",
    "@type": "WebSite",
    name: "iii",
    alternateName: "iii.dev",
    url: `${site.url}/`,
    publisher: { "@type": "Organization", name: "III, Inc.", url: `${site.url}/` },
  },
  {
    "@context": "https://schema.org",
    "@type": "SoftwareApplication",
    name: "iii",
    applicationCategory: "DeveloperApplication",
    operatingSystem: "Linux, macOS, Windows",
    description:
      "An engine and a single open protocol (JSON over WebSocket) built on three primitives: Function, Trigger, Worker. Any process, in any language, on any runtime, that speaks the protocol joins as a worker. Unix made everything a file. React made everything a component. iii makes everything a worker.",
    url: `${site.url}/`,
    downloadUrl: links.github,
    license: "https://www.elastic.co/licensing/elastic-license",
    programmingLanguage: ["Rust", "TypeScript", "Python"],
    offers: { "@type": "Offer", price: "0", priceCurrency: "USD" },
    author: { "@type": "Organization", name: "III, Inc.", url: `${site.url}/` },
  },
]
