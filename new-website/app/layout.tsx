import { GeistPixelCircle } from 'geist/font/pixel'
import type { Metadata, Viewport } from 'next'
import { Geist_Mono, Inter } from 'next/font/google'

import './globals.css'
import { MotionProvider } from '@/components/site/motion-provider'
import { ThemeProvider } from '@/components/theme-provider'
import { site } from '@/lib/seo'
import { cn } from '@/lib/utils'

const fontSans = Inter({ subsets: ['latin'], variable: '--font-sans' })

const fontMono = Geist_Mono({
  subsets: ['latin'],
  variable: '--font-mono',
})

/* Site-wide defaults. Pages override the title, description and canonical; Open Graph and Twitter images come
   from app/opengraph-image.png and app/twitter-image.png (the live iii.dev share card). */
export const metadata: Metadata = {
  metadataBase: new URL(site.url),
  title: { default: site.title, template: `%s | ${site.name}` },
  description: site.description,
  applicationName: site.name,
  keywords: [...site.keywords],
  authors: [{ name: 'III, Inc.', url: site.url }],
  creator: 'III, Inc.',
  publisher: 'III, Inc.',
  category: 'technology',
  alternates: { canonical: '/' },
  robots: {
    index: true,
    follow: true,
    googleBot: { index: true, follow: true, 'max-image-preview': 'large', 'max-snippet': -1, 'max-video-preview': -1 },
  },
  openGraph: {
    type: 'website',
    siteName: site.name,
    locale: site.locale,
    url: '/',
    title: site.title,
    description: site.description,
  },
  twitter: {
    card: 'summary_large_image',
    site: site.twitter,
    creator: site.twitter,
    title: site.title,
    description: site.description,
  },
  formatDetection: { telephone: false, email: false, address: false },
}

export const viewport: Viewport = {
  themeColor: '#0b0b0b',
  colorScheme: 'dark',
}

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode
}>) {
  return (
    <html
      lang="en"
      suppressHydrationWarning
      className={cn('dark antialiased', fontSans.variable, fontMono.variable, GeistPixelCircle.variable)}
    >
      <body>
        <ThemeProvider>
          <MotionProvider>{children}</MotionProvider>
        </ThemeProvider>
      </body>
    </html>
  )
}
