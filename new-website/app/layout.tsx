import { GeistPixelCircle } from 'geist/font/pixel'
import type { Metadata, Viewport } from 'next'
import { Geist_Mono, Inter } from 'next/font/google'

import './globals.css'
import { MotionProvider } from '@/components/site/motion-provider'
import { ThemeProvider } from '@/components/theme-provider'
import { cn } from '@/lib/utils'

const fontSans = Inter({ subsets: ['latin'], variable: '--font-sans' })

const fontMono = Geist_Mono({
  subsets: ['latin'],
  variable: '--font-mono',
})

export const metadata: Metadata = {
  metadataBase: new URL('https://iii.dev'),
  title: 'iii: anything that opens a WebSocket can join your backend',
  description:
    'iii is an engine your APIs, jobs, devices, and AI agents all connect to. Each one registers functions by name, and any other can call them, react to them, and trace them.',
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
