'use client'

import { ThemeProvider as NextThemesProvider } from 'next-themes'
import type * as React from 'react'

/** The site ships dark only; the light tokens stay in globals.css so a switch could come back later. */
function ThemeProvider({ children, ...props }: React.ComponentProps<typeof NextThemesProvider>) {
  return (
    <NextThemesProvider attribute="class" forcedTheme="dark" defaultTheme="dark" enableSystem={false} {...props}>
      {children}
    </NextThemesProvider>
  )
}

export { ThemeProvider }
