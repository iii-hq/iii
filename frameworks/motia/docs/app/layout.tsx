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
          className="sticky top-0 z-50 w-full border-b-2 border-amber-500 bg-gradient-to-r from-amber-500 via-orange-500 to-red-500 text-white shadow-lg"
        >
          <div className="mx-auto flex max-w-screen-xl flex-col items-center gap-3 px-4 py-4 text-center sm:flex-row sm:gap-6 sm:py-5 sm:text-left">
            <span className="inline-flex shrink-0 items-center gap-2 rounded-full bg-white/20 px-3 py-1 text-xs font-bold uppercase tracking-widest ring-1 ring-white/40">
              <span aria-hidden>⚠️</span>
              Deprecated
            </span>
            <div className="flex-1 text-base font-semibold leading-snug sm:text-lg">
              <span className="block sm:inline">Motia is now deprecated.</span>{' '}
              <span className="block font-bold sm:inline">
                Say hello to{' '}
                <Link
                  href="https://iii.dev"
                  className="underline decoration-2 underline-offset-4 hover:text-amber-50"
                >
                  iii.dev
                </Link>
                .
              </span>
            </div>
            <div className="flex shrink-0 flex-col gap-2 sm:flex-row sm:items-center">
              <Link
                href="https://blog.motia.dev/motia-helped-us-build-something-incredible/"
                target="_blank"
                rel="noopener noreferrer"
                className="inline-flex items-center justify-center rounded-md bg-white/15 px-4 py-2 text-sm font-bold text-white ring-1 ring-white/40 transition hover:bg-white/25"
              >
                Read the blog →
              </Link>
              <Link
                href="https://iii.dev/docs/changelog/0-11-0/migrating-from-motia-js"
                className="inline-flex items-center justify-center rounded-md bg-white px-4 py-2 text-sm font-bold text-red-600 shadow-sm transition hover:bg-amber-50"
              >
                View migration guide →
              </Link>
            </div>
          </div>
        </div>
        <RootProvider>{children}</RootProvider>
      </body>
    </html>
  )
}
