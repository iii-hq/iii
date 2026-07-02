import { ModeToggle } from '@lib/components/schematic/ModeToggle'
import { Wordmark } from '@lib/components/schematic/Wordmark'
import { useTheme } from '@lib/hooks/useTheme'
import { SITE } from './site'

export function SiteHeader() {
  const [theme, setTheme] = useTheme()

  return (
    <header className="sticky top-0 z-50 border-b border-rule bg-bg">
      <div className="flex items-center gap-x-5 px-4 py-2.5 @3xl:px-9">
        <a href="./" className="flex items-center gap-x-2.5 shrink-0" aria-label="back to the index">
          <Wordmark />
          <span className="font-mono text-[13px] font-semibold lowercase text-ink hidden @lg:inline">
            {SITE.wordmarkLabel}
          </span>
        </a>

        <span className="hidden @3xl:inline font-mono text-[12px] lowercase text-ink-ghost">
          the roadmap, spec by spec
        </span>

        <div className="ml-auto shrink-0">
          <ModeToggle
            value={theme}
            onChange={setTheme}
            options={[
              { value: 'light', label: 'light' },
              { value: 'dark', label: 'dark' },
            ]}
          />
        </div>
      </div>
    </header>
  )
}
