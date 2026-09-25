// The privacy policy, word for word from the pre-Astro privacy-policy.html
// (see website/src/components/privacy/body.html). Only the design lives in
// page.tsx. `body` is markdown for `renderMarkdown`, which gives each heading
// an id for the contents list.

export const privacy = {
  eyebrow: "Legal",
  title: "Privacy policy",
  /** the source's "Last updated" line, surfaced in the page head as well */
  updated: "June 2026",
  lede: "Motia LLC operates the iii.dev website and the open-source iii engine. This policy explains what information we collect through the website, how we use it, and the choices you have. The iii engine runs on your own infrastructure and does not send your code, data, or project information to us.",
} as const

export const body = `
## Information we collect

When you visit the iii.dev website, our hosting and analytics providers record standard log data that your browser sends: your IP address, browser type and version, the pages you view, the date and time of your visit, and similar diagnostics. We use this information in aggregate to operate and improve the site.

You do not need an account to use iii, and we do not require or collect your name or other contact details to download or run it.

## Cookies and analytics

Non-essential analytics load only after you accept the cookie banner. You can decline at the banner, or clear your choice in your browser at any time, and these services will not load. We use the following providers:

- **Google Tag Manager:** loads and manages the tags listed below.
- **PostHog:** product analytics, including page views and aggregate interactions.
- **Common Room:** measures how visitors find and engage with the site.

## How we use information

We use the information we collect to:

- Operate, maintain, and secure the iii.dev website and documentation.
- Understand how visitors find and use the site, in aggregate.
- Respond to questions and requests you send us.
- Detect, prevent, and address technical or security issues.

We do not sell your personal data or use it for advertising.

## Service providers and sharing

We share information only in limited circumstances:

- **Service providers:** third parties that host the site and provide the analytics described above. They may access this information only to perform tasks on our behalf and are obligated not to use it for any other purpose.
- **Legal requirements:** when required by law, or to protect our rights and the safety of users.
- **Business transfers:** in connection with a merger, acquisition, or sale of assets, with notice to affected users.

## Data security

We use commercially reasonable measures to protect the information we hold. No method of transmission or storage over the internet is fully secure, so we cannot guarantee absolute security, but we will notify users of any significant breach as required by law.

## Your rights and choices

Depending on where you live, you may have the right to access, correct, or delete the personal information we hold about you, and you can decline analytics at any time through the cookie banner. To make a request, contact us using the details below.

## International data transfers

Our service providers may process information in countries other than your own. Where that happens, we rely on appropriate safeguards, such as standard contractual clauses, for the transfer.

## Children's privacy

The website is not directed to children under 13, and we do not knowingly collect personal information from them. If you believe a child has provided us with personal information, contact us and we will delete it.

## Changes to this policy

We may update this policy from time to time. We will post any changes on this page and update the "Last updated" date below.

## Contact

Owner and data controller: Motia LLC.

<p class="doc-updated">Last updated: ${privacy.updated}</p>
`
