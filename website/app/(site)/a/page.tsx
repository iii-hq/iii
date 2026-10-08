import type { Metadata } from 'next'

/**
 * /a was variant A of the landing page test; it is the homepage now. The static export has no server redirects, so
 * this page refreshes to "/" (React hoists the meta tag into the head) and links there for anyone it does not move.
 */
export const metadata: Metadata = {
  title: { absolute: 'iii' },
  robots: { index: false, follow: true },
  alternates: { canonical: '/' },
}

export default function Page() {
  return (
    <main className="mx-auto max-w-[560px] px-5 pt-[20vh] text-[15px] text-muted-foreground">
      <meta httpEquiv="refresh" content="0; url=/" />
      <p>
        This page moved to{' '}
        <a href="/" className="text-foreground underline underline-offset-4">
          iii.dev
        </a>
        .
      </p>
    </main>
  )
}
