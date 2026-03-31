import { useState, useEffect } from 'react';
import { ChevronDown } from 'lucide-react';
import { GithubIcon } from '../icons';
import { InstallShButton } from '../InstallShButton';
import { EmailSignupForm } from '../EmailSignupForm';
import { Logo } from '../Logo';
import { TextParticle, drawIiiLogo } from '../ui/text-particle';

// Discord integration
const DISCORD_GUILD_ID = '1322278831184281721';
const DISCORD_WIDGET_URL = `https://discord.com/api/guilds/${DISCORD_GUILD_ID}/widget.json`;
const DISCORD_INVITE_URL = 'https://discord.gg/motia';

interface DiscordMember {
  id: string;
  username: string;
  avatar_url: string;
  status: string;
}

interface DiscordStats {
  onlineCount: number;
  serverName: string;
  members: DiscordMember[];
  isLoading: boolean;
  error: string | null;
  inviteUrl: string | null;
}

const fallbackAvatars = ['👨‍💻', '👩‍💼', '👨‍🔬', '👩‍💻', '👨‍💼', '👩‍🔬'];

function useDiscordWidget(): DiscordStats {
  const [stats, setStats] = useState<DiscordStats>({
    onlineCount: 0,
    serverName: 'iii Community',
    members: [],
    isLoading: true,
    error: null,
    inviteUrl: DISCORD_INVITE_URL,
  });

  useEffect(() => {
    const fetchDiscordData = async () => {
      try {
        const response = await fetch(DISCORD_WIDGET_URL);
        if (!response.ok) {
          throw new Error(
            response.status === 403
              ? 'Widget is disabled for this server'
              : `Failed to fetch: ${response.status}`,
          );
        }
        const data = await response.json();
        setStats({
          onlineCount: data.presence_count,
          serverName: data.name,
          members: data.members?.slice(0, 8) || [],
          isLoading: false,
          error: null,
          inviteUrl: data.instant_invite || DISCORD_INVITE_URL,
        });
      } catch (err) {
        setStats((prev) => ({
          ...prev,
          isLoading: false,
          error: err instanceof Error ? err.message : 'Offline',
          inviteUrl: DISCORD_INVITE_URL,
        }));
      }
    };
    fetchDiscordData();
    const interval = setInterval(fetchDiscordData, 5 * 60 * 1000);
    return () => clearInterval(interval);
  }, []);

  return stats;
}

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
      'Active development. Join Discord for early access and to shape what ships next.',
  },
];

interface FooterSectionProps {
  isDarkMode?: boolean;
}

export function FooterSection({ isDarkMode = true }: FooterSectionProps) {
  const [openFaqId, setOpenFaqId] = useState<number | null>(null);
  const discord = useDiscordWidget();

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
              Install the engine, join the community, check out our code, or
              subscribe for updates
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
          <div className={ctaGrid}>
            <a
              href="https://github.com/iii-hq/iii"
              target="_blank"
              rel="noopener noreferrer"
              className={`${ctaButtonBase} ${
                isDarkMode
                  ? 'bg-iii-dark/50 border-iii-light hover:border-iii-light text-iii-light'
                  : 'bg-white/50 border-iii-dark hover:border-iii-dark text-iii-black'
              }`}
            >
              <GithubIcon size={16} />
              GitHub
            </a>
            <div className="relative">
              <button
                onClick={() =>
                  window.open(
                    discord.inviteUrl || DISCORD_INVITE_URL,
                    '_blank',
                    'noopener,noreferrer',
                  )
                }
                className={`${ctaButtonBase} bg-[#5865F2] border-[#5865F2] hover:bg-[#4752C4] hover:border-[#4752C4] text-white`}
              >
                <svg
                  className="w-4 h-4"
                  fill="currentColor"
                  viewBox="0 0 127.14 96.36"
                >
                  <path d="M107.7,8.07A105.15,105.15,0,0,0,81.47,0a72.06,72.06,0,0,0-3.36,6.83A97.68,97.68,0,0,0,49,6.83,72.37,72.37,0,0,0,45.64,0,105.89,105.89,0,0,0,19.39,8.09C2.79,32.65-1.71,56.6.54,80.21h0A105.73,105.73,0,0,0,32.71,96.36,77.7,77.7,0,0,0,39.6,85.25a68.42,68.42,0,0,1-10.85-5.18c.91-.66,1.8-1.34,2.66-2a75.57,75.57,0,0,0,64.32,0c.87.71,1.76,1.39,2.66,2a68.68,68.68,0,0,1-10.87,5.19,77,77,0,0,0,6.89,11.1A105.25,105.25,0,0,0,126.6,80.22h0C129.24,52.84,122.09,29.11,107.7,8.07ZM42.45,65.69C36.18,65.69,31,60,31,53s5-12.74,11.43-12.74S54,46,53.89,53,48.84,65.69,42.45,65.69Zm42.24,0C78.41,65.69,73.25,60,73.25,53s5-12.74,11.44-12.74S96.23,46,96.12,53,91.08,65.69,84.69,65.69Z" />
                </svg>
                <span>Discord</span>
                {!discord.error && !discord.isLoading && (
                  <>
                    <span className="w-1.5 h-1.5 rounded-full bg-iii-success" />
                    <span className="font-normal opacity-80">
                      {discord.onlineCount} online
                    </span>
                  </>
                )}
              </button>
              {!discord.error && (
                <div className="absolute left-0 right-0 flex items-center justify-center gap-1 mt-2">
                  {(discord.members.length > 0
                    ? discord.members
                    : fallbackAvatars.map((emoji, i) => ({
                        id: `fb-${i}`,
                        username: '',
                        avatar_url: '',
                        status: 'online',
                        emoji,
                      }))
                  )
                    .slice(0, 6)
                    .map((member, index) => (
                      <div
                        key={member.id}
                        className="relative"
                        style={{
                          marginLeft: index > 0 ? '-6px' : '0',
                          zIndex: 10 - index,
                        }}
                      >
                        {member.avatar_url ? (
                          <img
                            src={member.avatar_url}
                            alt={member.username}
                            className="w-7 h-7 rounded-full border-2 border-iii-dark object-cover"
                          />
                        ) : (
                          <div
                            className={`w-7 h-7 rounded-full border-2 flex items-center justify-center text-xs ${isDarkMode ? 'border-iii-dark bg-iii-dark/50' : 'border-white bg-gray-100'}`}
                          >
                            {(member as { emoji?: string }).emoji || '👤'}
                          </div>
                        )}
                        <span
                          className={`absolute -bottom-0.5 -right-0.5 w-2.5 h-2.5 rounded-full border-2 ${isDarkMode ? 'border-iii-dark' : 'border-white'} ${
                            member.status === 'online'
                              ? 'bg-iii-success'
                              : member.status === 'idle'
                                ? 'bg-yellow-500'
                                : member.status === 'dnd'
                                  ? 'bg-red-500'
                                  : 'bg-gray-500'
                          }`}
                        />
                      </div>
                    ))}
                  {discord.onlineCount > 6 && (
                    <span className={`text-xs ml-2 ${textSecondary}`}>
                      +{discord.onlineCount - 6} more
                    </span>
                  )}
                </div>
              )}
            </div>
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
