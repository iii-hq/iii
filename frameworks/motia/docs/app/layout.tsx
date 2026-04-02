import type { Metadata } from 'next'
import { Geist } from 'next/font/google'
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
        <RootProvider>{children}</RootProvider>
      </body>
    </html>
  )
}
