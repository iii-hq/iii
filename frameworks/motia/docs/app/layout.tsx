import type { Metadata } from 'next'
import { Geist } from 'next/font/google'
import Link from 'next/link'
import './globals.css'
import { RootProvider } from 'fumadocs-ui/provider'

const geistSans = Geist({
  weight: ['400', '500', '600'],
  variable: '--font-geist-sans',
  subsets: ['latin'],
})

export const metadata: Metadata = {
  title: {
    default: 'Motia Docs',
    template: '%s | Motia Docs',
  },
  description: 'Documentation for Motia - the iii engine',
  icons: {
    icon: '/favicon.png',
  },
}

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode
}>) {
  return (
    <html lang="en" suppressHydrationWarning>
      <body className={`${geistSans.variable} antialiased`}>
        <div
          role="alert"
          className="sticky top-0 z-50 w-full border-b border-white/10 bg-black/95 text-white backdrop-blur supports-[backdrop-filter]:bg-black/80"
        >
          <div className="h-px w-full bg-gradient-to-r from-transparent via-amber-400/70 to-transparent" />
          <div className="mx-auto flex max-w-6xl flex-col items-center gap-4 px-6 py-3.5 text-center sm:flex-row sm:gap-6 sm:text-left">
            <span className="inline-flex shrink-0 items-center gap-2 rounded-full border border-amber-400/40 bg-amber-400/10 px-2.5 py-0.5 font-mono text-[11px] font-medium uppercase tracking-[0.18em] text-amber-300">
              <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-amber-400" aria-hidden />
              Deprecated
            </span>
            <p className="flex-1 text-[13px] leading-relaxed text-zinc-200 sm:text-sm">
              <span className="text-white">Motia is now deprecated.</span>{' '}
              <span className="text-zinc-400">Say hello to</span>{' '}
              <Link
                href="https://iii.dev"
                className="font-semibold text-white underline decoration-amber-400/70 decoration-2 underline-offset-[5px] transition hover:decoration-amber-300"
              >
                iii.dev
              </Link>
              .
            </p>
            <div className="flex shrink-0 items-center gap-1.5 font-mono text-[12px]">
              <Link
                href="https://blog.motia.dev/motia-helped-us-build-something-incredible/"
                target="_blank"
                rel="noopener noreferrer"
                className="rounded-md border border-white/15 px-3 py-1.5 text-zinc-200 transition hover:border-white/30 hover:text-white"
              >
                Read the blog
              </Link>
              <Link
                href="https://iii.dev/docs/changelog/0-11-0/migrating-from-motia-js"
                className="rounded-md bg-white px-3 py-1.5 font-medium text-black transition hover:bg-zinc-200"
              >
                Migration guide →
              </Link>
            </div>
          </div>
        </div>
        <RootProvider>{children}</RootProvider>
      </body>
    </html>
  )
}
