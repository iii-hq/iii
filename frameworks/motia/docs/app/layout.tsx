import type { Metadata } from 'next'
import { Geist } from 'next/font/google'
import Link from 'next/link'
import './globals.css'
import { Banner } from 'fumadocs-ui/components/banner'
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
        <Banner id="motia-deprecation" variant="rainbow" changeLayout>
          <span>
            Motia is being deprecated. Say hello to{' '}
            <Link href="https://iii.dev" className="font-semibold underline">
              iii.dev
            </Link>
            !{' '}
            <Link
              href="https://blog.motia.dev/motia-helped-us-build-something-incredible/"
              className="font-semibold underline"
              target="_blank"
              rel="noopener noreferrer"
            >
              Read the blog
            </Link>
            <span className="opacity-60"> • </span>
            <Link
              href="https://iii.dev/docs/changelog/0-11-0/migrating-from-motia-js"
              className="font-semibold underline"
            >
              View the migration guide
            </Link>
          </span>
        </Banner>
        <RootProvider>{children}</RootProvider>
      </body>
    </html>
  )
}
