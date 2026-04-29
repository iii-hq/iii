import { EmailSignupForm } from '../EmailSignupForm'
import { InstallShButton } from '../InstallShButton'
import { GithubIcon } from '../icons'
import { Logo } from '../Logo'

interface FooterSectionProps {
  isDarkMode?: boolean
}

export function FooterSection({ isDarkMode = true }: FooterSectionProps) {
  const textPrimary = isDarkMode ? 'text-iii-light' : 'text-iii-black'
  const textSecondary = isDarkMode ? 'text-iii-light/70' : 'text-iii-black/70'
  const ctaButtonBase =
    'group relative flex items-center justify-center gap-2 px-2.5 py-2 sm:px-3 sm:py-2.5 md:px-4 md:py-3 border rounded transition-colors cursor-pointer w-full text-[10px] sm:text-xs md:text-sm font-bold'
  const ctaGrid = 'grid grid-cols-1 sm:grid-cols-2 gap-3 sm:gap-4 md:gap-6 w-full max-w-2xl mx-auto px-2 sm:px-0'

  return (
    <footer className={`relative w-full overflow-hidden font-mono transition-colors duration-300 ${textPrimary}`}>
      <div className="relative z-10 w-full max-w-7xl mx-auto px-4 sm:px-6 py-8 md:py-12">
        {/* Primary CTA Section */}
        <div className="text-center mb-6 md:mb-8">
          <div className="mb-4">
            <h3 className={`text-xl md:text-2xl font-bold mb-2 ${textPrimary}`}>Get started!</h3>
            <p className={`text-sm ${textSecondary}`}>
              Install the engine, check out our code, or subscribe for updates
            </p>
          </div>

          <div className={`${ctaGrid} mb-4`}>
            <InstallShButton isDarkMode={isDarkMode} className="sm:w-full" />
            <EmailSignupForm isDarkMode={isDarkMode} showHelperText={false} className="sm:w-full" />
          </div>

          {/* CTA Buttons */}
          <div className="flex justify-center w-full max-w-2xl mx-auto px-2 sm:px-0">
            <a
              href="https://github.com/iii-hq/iii"
              target="_blank"
              rel="noopener noreferrer"
              className={`${ctaButtonBase} sm:max-w-xs ${
                isDarkMode
                  ? 'bg-iii-dark/50 border-iii-light hover:border-iii-light text-iii-light'
                  : 'bg-white/50 border-iii-dark hover:border-iii-dark text-iii-black'
              }`}
            >
              <GithubIcon size={16} />
              GitHub
            </a>
          </div>
        </div>

        <div className="pt-8 md:pt-12">
          <div className="flex flex-col md:flex-row items-center justify-between gap-4">
            <div className="flex items-center gap-3">
              <Logo className={`h-4 ${isDarkMode ? 'text-iii-light' : 'text-iii-black'}`} />
              <span className={`text-sm font-bold ${textPrimary}`}>Interoperable Invocation Interface</span>
            </div>
            <div className={`text-xs ${textSecondary}`}>© Motia LLC</div>
          </div>
        </div>
      </div>
    </footer>
  )
}
