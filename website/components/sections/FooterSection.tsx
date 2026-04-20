import { useState } from 'react';
import { ChevronDown } from 'lucide-react';
import { GithubIcon } from '../icons';
import { InstallShButton } from '../InstallShButton';
import { EmailSignupForm } from '../EmailSignupForm';
import { Logo } from '../Logo';
import { TextParticle, drawIiiLogo } from '../ui/text-particle';

// Top 5 FAQ questions as specified in the plan
const faqItems = [
  {
    id: 1,
    question: 'How is iii different from gRPC?',
    answer:
      'gRPC needs compile-time IDL and codegen. iii uses runtime registration — functions available the moment a worker connects.',
  },
  {
    id: 2,
    question: 'How is iii different from a service mesh?',
    answer:
      'Service meshes need sidecars and complex networking. iii is one binary — workers connect via WebSocket, nothing else.',
  },
  {
    id: 3,
    question: 'Can I use iii with my existing Express/Flask/Spring app?',
    answer:
      'Yes. Add the SDK, register routes as functions. They join the distributed architecture instantly. Incremental adoption.',
  },
  {
    id: 4,
    question: 'What about AI agents and LLMs?',
    answer:
      'Functions self-describe with schemas. Agents discover and trigger them autonomously. Everything is auto-generated.',
  },
  {
    id: 5,
    question: 'Is iii production-ready?',
    answer:
      'Active development. Follow GitHub for early access and to shape what ships next.',
  },
];

interface FooterSectionProps {
  isDarkMode?: boolean;
}

export function FooterSection({ isDarkMode = true }: FooterSectionProps) {
  const [openFaqId, setOpenFaqId] = useState<number | null>(null);

  const textPrimary = isDarkMode ? 'text-iii-light' : 'text-iii-black';
  const textSecondary = isDarkMode ? 'text-iii-light/70' : 'text-iii-black/70';
  const borderColor = isDarkMode
    ? 'border-iii-light/10'
    : 'border-iii-black/10';
  const bgCard = isDarkMode ? 'bg-iii-dark/20' : 'bg-white/40';
  const accentColor = isDarkMode ? 'text-iii-accent' : 'text-iii-accent-light';
  const accentBorder = isDarkMode
    ? 'border-iii-accent'
    : 'border-iii-accent-light';
  const ctaButtonBase =
    'group relative flex items-center justify-center gap-2 px-2.5 py-2 sm:px-3 sm:py-2.5 md:px-4 md:py-3 border rounded transition-colors cursor-pointer w-full text-[10px] sm:text-xs md:text-sm font-bold';
  const ctaGrid =
    'grid grid-cols-1 sm:grid-cols-2 gap-3 sm:gap-4 md:gap-6 w-full max-w-2xl mx-auto px-2 sm:px-0';

  return (
    <footer
      className={`relative w-full overflow-hidden font-mono transition-colors duration-300 ${textPrimary}`}
    >
      <div className="relative z-10 w-full max-w-7xl mx-auto px-4 sm:px-6 py-8 md:py-12">
        {/* Primary CTA Section */}
        <div className="text-center mb-6 md:mb-8">
          <div className="mb-4">
            <h3 className={`text-xl md:text-2xl font-bold mb-2 ${textPrimary}`}>
              Get started!
            </h3>
            <p className={`text-sm ${textSecondary}`}>
              Install the engine, check out our code, or subscribe for updates
            </p>
          </div>

          <div className={`${ctaGrid} mb-4`}>
            <InstallShButton isDarkMode={isDarkMode} className="sm:w-full" />
            <EmailSignupForm
              isDarkMode={isDarkMode}
              showHelperText={false}
              className="sm:w-full"
            />
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

        {/* FAQ — hidden for now */}
        {/* <div className="mb-6 md:mb-8">
          <div>
            <h4
              className={`text-xs font-semibold uppercase tracking-wider mb-4 ${accentColor}`}
            >
              FAQ
            </h4>
            <div className="space-y-2">
              {faqItems.map((item) => (
                <div
                  key={item.id}
                  className={`
                    rounded-lg transition-all duration-300 overflow-hidden
                    ${
                      openFaqId === item.id
                        ? `border-2 ${accentBorder} ${isDarkMode ? 'bg-iii-accent/5' : 'bg-iii-accent-light/5'}`
                        : `border ${borderColor} ${bgCard} hover:pl-5`
                    }
                  `}
                >
                  <button
                    onClick={() =>
                      setOpenFaqId(openFaqId === item.id ? null : item.id)
                    }
                    className="w-full text-left px-4 py-3 flex items-center gap-3"
                  >
                    <span
                      className={`text-xs font-bold ${openFaqId === item.id ? accentColor : textSecondary}`}
                    >
                      {String(item.id).padStart(2, '0')}
                    </span>
                    <span
                      className={`flex-1 text-sm font-medium ${textPrimary}`}
                    >
                      {item.question}
                    </span>
                    <ChevronDown
                      className={`
                      w-4 h-4 transition-transform duration-300
                      ${openFaqId === item.id ? `rotate-180 ${accentColor}` : textSecondary}
                    `}
                    />
                  </button>
                  <div
                    className={`
                    overflow-hidden transition-all duration-300
                    ${openFaqId === item.id ? 'max-h-40 opacity-100' : 'max-h-0 opacity-0'}
                  `}
                  >
                    <p
                      className={`px-4 pb-3 pl-8 sm:pl-12 text-xs leading-relaxed ${textSecondary}`}
                    >
                      {item.answer}
                    </p>
                  </div>
                </div>
              ))}
            </div>
          </div>
        </div> */}

        {/* iii Particle Branding — hidden for now */}
        {/* <div className={`border-t ${borderColor} pt-8 md:pt-12`}>
          <div className="h-52 sm:h-72 md:h-96 lg:h-[28rem] w-full overflow-hidden mb-8">
            <TextParticle
              renderSource={drawIiiLogo}
              particleColor={isDarkMode ? '#f4f4f4' : '#000000'}
              hoverColor={isDarkMode ? '#f3f724' : '#2f7fff'}
              hoverRadius={150}
              particleSize={2.5}
              particleDensity={4}
            />
          </div>
        </div> */}

        <div className="pt-8 md:pt-12">
          <div className="flex flex-col md:flex-row items-center justify-between gap-4">
            <div className="flex items-center gap-3">
              <Logo
                className={`h-4 ${isDarkMode ? 'text-iii-light' : 'text-iii-black'}`}
              />
              <span className={`text-sm font-bold ${textPrimary}`}>
                Interoperable Invocation Interface
              </span>
            </div>
            <div className={`text-xs ${textSecondary}`}>© Motia LLC</div>
          </div>
        </div>
      </div>
    </footer>
  );
}
