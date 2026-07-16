import { useCallback, useEffect, useState } from 'react'

export type Theme = 'light' | 'dark'

function readTheme(): Theme {
  // The document attribute is the source of truth — the shared ThemeInit
  // (site-wide 'iii_theme' key + prefers-color-scheme) applies it pre-paint.
  const attr = document.documentElement.dataset.theme
  if (attr === 'dark' || attr === 'light') return attr
  try {
    const t = localStorage.getItem('iii_theme')
    return t === 'dark' ? 'dark' : 'light'
  } catch {
    return 'light'
  }
}

export function useTheme(): [Theme, (next: Theme) => void] {
  const [theme, setTheme] = useState<Theme>(readTheme)

  useEffect(() => {
    document.documentElement.dataset.theme = theme
  }, [theme])

  const set = useCallback((next: Theme) => {
    setTheme(next)
    try {
      localStorage.setItem('iii_theme', next)
    } catch {
      /* private mode — theme just won't persist */
    }
  }, [])

  return [theme, set]
}
