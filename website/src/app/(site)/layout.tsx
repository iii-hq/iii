import type { Metadata, Viewport } from "next"
import { Geist_Mono, Inter } from "next/font/google"
import type { ReactNode } from "react"
import { preconnect } from "react-dom"
import { MotionProvider } from "@/components/motion/provider"
import { AnalyticsHead, AnalyticsNoScript } from "@/components/site/analytics"
import { CookieBanner } from "@/components/site/cookie-banner"
import { SiteHeader } from "@/components/site/header/site-header"
import { SiteFooter } from "@/components/site/site-footer"
import { ThemeScript } from "@/components/site/theme-script"
import { site } from "@/lib/site"
import { THEME_COLOR } from "@/lib/theme"
import "./globals.css"

const inter = Inter({
  subsets: ["latin"],
  variable: "--font-inter",
  display: "swap",
  // Inter 4's optical-size axis: display cuts at headline sizes, text cuts in body.
  axes: ["opsz"],
})

// Code, terminals and transcripts.
const geistMono = Geist_Mono({
  subsets: ["latin"],
  variable: "--font-geist-mono",
  display: "swap",
})

export const metadata: Metadata = {
  metadataBase: new URL(site.url),
  title: site.title,
  description: site.description,
  keywords: [...site.keywords],
  authors: [{ name: "III, Inc." }],
  robots: { index: true, follow: true },
  icons: { icon: "/favicon.svg", apple: "/favicon.svg" },
  openGraph: {
    type: "website",
    url: "/",
    title: site.title,
    description: site.description,
    images: [{ url: "/og-image.png", width: 1200, height: 630, type: "image/png" }],
  },
  twitter: {
    card: "summary_large_image",
    title: site.title,
    description: site.description,
    images: ["/og-image.png"],
  },
}

export const viewport: Viewport = {
  themeColor: THEME_COLOR.dark,
}

export default function RootLayout({ children }: { children: ReactNode }) {
  // The header asks GitHub and Discord for counts on first paint; open those connections with the document.
  preconnect("https://api.github.com")
  preconnect("https://discord.com")
  return (
    // The theme script mutates <html> before hydration, so React must accept the DOM's class.
    <html lang="en" className={`${inter.variable} ${geistMono.variable} dark`} suppressHydrationWarning>
      <head>
        <ThemeScript />
        <AnalyticsHead />
      </head>
      <body>
        <AnalyticsNoScript />
        <MotionProvider>
          {/* Header and footer are part of the frame, not the page, so a route change swaps only what's between them. */}
          <SiteHeader />
          {children}
          <SiteFooter />
          <CookieBanner />
        </MotionProvider>
      </body>
    </html>
  )
}
