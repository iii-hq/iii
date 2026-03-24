import { useCallback, useEffect, useState } from 'react'

type Theme = 'dark' | 'light'

function getStoredTheme(): Theme {
  try {
    const stored = localStorage.getItem('iii-theme')
    if (stored === 'light') return 'light'
  } catch {
    // private browsing - default to dark
  }
  return 'dark'
}

export function useTheme() {
  const [theme, setThemeState] = useState<Theme>(getStoredTheme)

  useEffect(() => {
    if (theme === 'light') {
      document.documentElement.setAttribute('data-theme', 'light')
    } else {
      document.documentElement.removeAttribute('data-theme')
    }
  }, [theme])

  const setTheme = useCallback((t: Theme) => {
    setThemeState(t)
    try {
      localStorage.setItem('iii-theme', t)
    } catch {
      // private browsing throws DOMException
    }
  }, [])

  const toggleTheme = useCallback(() => {
    // Enable transition on all elements during theme change
    document.documentElement.setAttribute('data-theme-transitioning', '')
    setTheme(theme === 'dark' ? 'light' : 'dark')
    // Remove after transitions complete
    setTimeout(() => {
      document.documentElement.removeAttribute('data-theme-transitioning')
    }, 250)
  }, [theme, setTheme])

  return { theme, setTheme, toggleTheme }
}
