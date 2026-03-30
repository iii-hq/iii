import React, { useState, useRef, useEffect } from 'react';
import { Highlight, themes } from 'prism-react-renderer';
import {
  Database,
  Layers,
  GitBranch,
  Zap,
  Activity,
  Share2,
  Bot,
  ArrowRight,
  ChevronDown,
} from 'lucide-react';
import {
  GlobeIcon,
  ClockIcon,
  MessageCircleIcon,
  EyeIcon,
  Cloud1Icon as CloudIcon,
} from '../icons';
import { Logo } from '../Logo';
import { AnimatedConnector } from './AnimatedConnector';

interface EngineSectionProps {
  isDarkMode?: boolean;
}

type Lang = 'typescript' | 'python' | 'rust';

const LangIcon = ({ lang, active }: { lang: Lang; active: boolean }) => {
  const opacity = active ? 1 : 0.5;
  const size = 'w-4 h-4';

  if (lang === 'typescript') {
    return (
      <svg viewBox="0 0 24 24" className={size} style={{ opacity }}>
        <rect width="24" height="24" rx="2" fill="#3178c6" />
        <path
          d="M1.125 0C.502 0 0 .502 0 1.125v21.75C0 23.498.502 24 1.125 24h21.75c.623 0 1.125-.502 1.125-1.125V1.125C24 .502 23.498 0 22.875 0zm17.363 9.75c.612 0 1.154.037 1.627.111a6.38 6.38 0 0 1 1.306.34v2.458a3.95 3.95 0 0 0-.643-.361 5.093 5.093 0 0 0-.717-.26 5.453 5.453 0 0 0-1.426-.2c-.3 0-.573.028-.819.086a2.1 2.1 0 0 0-.623.242c-.17.104-.3.229-.393.374a.888.888 0 0 0-.14.49c0 .196.053.373.156.529.104.156.252.304.443.444s.423.276.696.41c.273.135.582.274.926.416.47.197.892.407 1.266.628.374.222.695.473.963.753.268.279.472.598.614.957.142.359.214.776.214 1.253 0 .657-.125 1.21-.373 1.656a3.033 3.033 0 0 1-1.012 1.085 4.38 4.38 0 0 1-1.487.596c-.566.12-1.163.18-1.79.18a9.916 9.916 0 0 1-1.84-.164 5.544 5.544 0 0 1-1.512-.493v-2.63a5.033 5.033 0 0 0 3.237 1.2c.333 0 .624-.03.872-.09.249-.06.456-.144.623-.25.166-.108.29-.234.373-.38a1.023 1.023 0 0 0-.074-1.089 2.12 2.12 0 0 0-.537-.5 5.597 5.597 0 0 0-.807-.444 27.72 27.72 0 0 0-1.007-.436c-.918-.383-1.602-.852-2.053-1.405-.45-.553-.676-1.222-.676-2.005 0-.614.123-1.141.369-1.582.246-.441.58-.804 1.004-1.089a4.494 4.494 0 0 1 1.47-.629 7.536 7.536 0 0 1 1.77-.201zm-15.113.188h9.563v2.166H9.506v9.646H6.789v-9.646H3.375z"
          fill="white"
        />
      </svg>
    );
  }

  if (lang === 'python') {
    return (
      <svg viewBox="0 0 24 24" className={size} style={{ opacity }}>
        <path
          d="M14.25.18l.9.2.73.26.59.3.45.32.34.34.25.34.16.33.1.3.04.26.02.2-.01.13V8.5l-.05.63-.13.55-.21.46-.26.38-.3.31-.33.25-.35.19-.35.14-.33.1-.3.07-.26.04-.21.02H8.77l-.69.05-.59.14-.5.22-.41.27-.33.32-.27.35-.2.36-.15.37-.1.35-.07.32-.04.27-.02.21v3.06H3.17l-.21-.03-.28-.07-.32-.12-.35-.18-.36-.26-.36-.36-.35-.46-.32-.59-.28-.73-.21-.88-.14-1.05-.05-1.23.06-1.22.16-1.04.24-.87.32-.71.36-.57.4-.44.42-.33.42-.24.4-.16.36-.1.32-.05.24-.01h.16l.06.01h8.16v-.83H6.18l-.01-2.75-.02-.37.05-.34.11-.31.17-.28.25-.26.31-.23.38-.2.44-.18.51-.15.58-.12.64-.1.71-.06.77-.04.84-.02 1.27.05zm-6.3 1.98l-.23.33-.08.41.08.41.23.34.33.22.41.09.41-.09.33-.22.23-.34.08-.41-.08-.41-.23-.33-.33-.22-.41-.09-.41.09zm13.09 3.95l.28.06.32.12.35.18.36.27.36.35.35.47.32.59.28.73.21.88.14 1.04.05 1.23-.06 1.23-.16 1.04-.24.86-.32.71-.36.57-.4.45-.42.33-.42.24-.4.16-.36.09-.32.05-.24.02-.16-.01h-8.22v.82h5.84l.01 2.76.02.36-.05.34-.11.31-.17.29-.25.25-.31.24-.38.2-.44.17-.51.15-.58.13-.64.09-.71.07-.77.04-.84.01-1.27-.04-1.07-.14-.9-.2-.73-.25-.59-.3-.45-.33-.34-.34-.25-.34-.16-.33-.1-.3-.04-.25-.02-.2.01-.13v-5.34l.05-.64.13-.54.21-.46.26-.38.3-.32.33-.24.35-.2.35-.14.33-.1.3-.06.26-.04.21-.02.13-.01h5.84l.69-.05.59-.14.5-.21.41-.28.33-.32.27-.35.2-.36.15-.36.1-.35.07-.32.04-.28.02-.21V6.07h2.09l.14.01zm-6.47 14.25l-.23.33-.08.41.08.41.23.33.33.23.41.08.41-.08.33-.23.23-.33.08-.41-.08-.41-.23-.33-.33-.23-.41-.08-.41.08z"
          fill="#3776AB"
        />
      </svg>
    );
  }

  return (
    <svg viewBox="0 0 24 24" className={size} style={{ opacity }}>
      <path
        d="M23.8346 11.7033l-1.0073-.6236a13.7268 13.7268 0 00-.0283-.2936l.8656-.8069a.3483.3483 0 00-.1154-.578l-1.1066-.414a8.4958 8.4958 0 00-.087-.2856l.6904-.9587a.3462.3462 0 00-.2257-.5446l-1.1663-.1894a9.3574 9.3574 0 00-.1407-.2622l.49-1.0761a.3437.3437 0 00-.0274-.3361.3486.3486 0 00-.3006-.154l-1.1845.0416a6.7444 6.7444 0 00-.1873-.2268l.2723-1.153a.3472.3472 0 00-.417-.4172l-1.1532.2724a14.0183 14.0183 0 00-.2278-.1873l.0415-1.1845a.3442.3442 0 00-.49-.328l-1.076.491c-.0872-.0476-.1742-.0952-.2623-.1407l-.1903-1.1673A.3483.3483 0 0016.256.955l-.9597.6905a8.4867 8.4867 0 00-.2855-.086l-.414-1.1066a.3483.3483 0 00-.5781-.1154l-.8069.8666a9.2936 9.2936 0 00-.2936-.0284L12.2946.1683a.3462.3462 0 00-.5892 0l-.6236 1.0073a13.7383 13.7383 0 00-.2936.0284L9.9803.3374a.3462.3462 0 00-.578.1154l-.4141 1.1065c-.0962.0274-.1903.0567-.2855.086L7.744.955a.3483.3483 0 00-.5447.2258L7.009 2.348a9.3574 9.3574 0 00-.2622.1407l-1.0762-.491a.3462.3462 0 00-.49.328l.0416 1.1845a7.9826 7.9826 0 00-.2278.1873L3.8413 3.425a.3472.3472 0 00-.4171.4171l.2713 1.1531c-.0628.075-.1255.1509-.1863.2268l-1.1845-.0415a.3462.3462 0 00-.328.49l.491 1.0761a9.167 9.167 0 00-.1407.2622l-1.1662.1894a.3483.3483 0 00-.2258.5446l.6904.9587a13.303 13.303 0 00-.087.2855l-1.1065.414a.3483.3483 0 00-.1155.5781l.8656.807a9.2936 9.2936 0 00-.0283.2935l-1.0073.6236a.3442.3442 0 000 .5892l1.0073.6236c.008.0982.0182.1964.0283.2936l-.8656.8079a.3462.3462 0 00.1155.578l1.1065.4141c.0273.0962.0567.1914.087.2855l-.6904.9587a.3452.3452 0 00.2268.5447l1.1662.1893c.0456.088.0922.1751.1408.2622l-.491 1.0762a.3462.3462 0 00.328.49l1.1834-.0415c.0618.0769.1235.1528.1873.2277l-.2713 1.1541a.3462.3462 0 00.4171.4161l1.153-.2713c.075.0638.151.1255.2279.1863l-.0415 1.1845a.3442.3442 0 00.49.327l1.0761-.49c.087.0486.1741.0951.2622.1407l.1903 1.1662a.3483.3483 0 00.5447.2268l.9587-.6904a9.299 9.299 0 00.2855.087l.414 1.1066a.3452.3452 0 00.5781.1154l.8079-.8656c.0972.0111.1954.0203.2936.0294l.6236 1.0073a.3472.3472 0 00.5892 0l.6236-1.0073c.0982-.0091.1964-.0183.2936-.0294l.8069.8656a.3483.3483 0 00.578-.1154l.4141-1.1066a8.4626 8.4626 0 00.2855-.087l.9587.6904a.3452.3452 0 00.5447-.2268l.1903-1.1662c.088-.0456.1751-.0931.2622-.1407l1.0762.49a.3472.3472 0 00.49-.327l-.0415-1.1845a6.7267 6.7267 0 00.2267-.1863l1.1531.2713a.3472.3472 0 00.4171-.416l-.2713-1.1542c.0628-.0749.1255-.1508.1863-.2278l1.1845.0415a.3442.3442 0 00.328-.49l-.49-1.076c.0475-.0872.0951-.1742.1407-.2623l1.1662-.1893a.3483.3483 0 00.2258-.5447l-.6904-.9587.087-.2855 1.1066-.414a.3462.3462 0 00.1154-.5781l-.8656-.8079c.0101-.0972.0202-.1954.0283-.2936l1.0073-.6236a.3442.3442 0 000-.5892zm-6.7413 8.3551a.7138.7138 0 01.2986-1.396.714.714 0 11-.2997 1.396zm-.3422-2.3142a.649.649 0 00-.7715.5l-.3573 1.6685c-1.1035.501-2.3285.7795-3.6193.7795a8.7368 8.7368 0 01-3.6951-.814l-.3574-1.6684a.648.648 0 00-.7714-.499l-1.473.3158a8.7216 8.7216 0 01-.7613-.898h7.1676c.081 0 .1356-.0141.1356-.088v-2.536c0-.074-.0536-.0881-.1356-.0881h-2.0966v-1.6077h2.2677c.2065 0 1.1065.0587 1.394 1.2088.0901.3533.2875 1.5044.4232 1.8729.1346.413.6833 1.2381 1.2685 1.2381h3.5716a.7492.7492 0 00.1296-.0131 8.7874 8.7874 0 01-.8119.9526zM6.8369 20.024a.714.714 0 11-.2997-1.396.714.714 0 01.2997 1.396zM4.1177 8.9972a.7137.7137 0 11-1.304.5791.7137.7137 0 011.304-.579zm-.8352 1.9813l1.5347-.6824a.65.65 0 00.33-.8585l-.3158-.7147h1.2432v5.6025H3.5669a8.7753 8.7753 0 01-.2834-3.348zm6.7343-.5437V8.7836h2.9601c.153 0 1.0792.1772 1.0792.8697 0 .575-.7107.7815-1.2948.7815zm10.7574 1.4862c0 .2187-.008.4363-.0243.651h-.9c-.09 0-.1265.0586-.1265.1477v.413c0 .973-.5487 1.1846-1.0296 1.2382-.4576.0517-.9648-.1913-1.0275-.4717-.2704-1.5186-.7198-1.8436-1.4305-2.4034.8817-.5599 1.799-1.386 1.799-2.4915 0-1.1936-.819-1.9458-1.3769-2.3153-.7825-.5163-1.6491-.6195-1.883-.6195H5.4682a8.7651 8.7651 0 014.907-2.7699l1.0974 1.151a.648.648 0 00.9182.0213l1.227-1.1743a8.7753 8.7753 0 016.0044 4.2762l-.8403 1.8982a.652.652 0 00.33.8585l1.6178.7188c.0283.2875.0425.577.0425.8717zm-9.3006-9.5993a.7128.7128 0 11.984 1.0316.7137.7137 0 01-.984-1.0316zm8.3389 6.71a.7107.7107 0 01.9395-.3625.7137.7137 0 11-.9405.3635z"
        fill="#dea584"
      />
    </svg>
  );
};

const concepts = [
  {
    id: 'function',
    icon: GitBranch,
    name: 'Functions',
    tagline: 'Anything that does work.',
    description:
      'A Function receives input and optionally returns output. It can live anywhere — locally, on cloud, on serverless, or as a third-party HTTP endpoint. All Functions are treated the same within iii.',
    highlights: [
      'Write in TypeScript, Python, or Rust — mix freely',
      'Addressable by path (users::create, orders::process)',
      'Hot-swap handlers without restarting consumers',
      'Auto-cleanup when workers disconnect',
    ],
    code: {
      typescript: `iii.registerFunction(
  { id: 'users::create' },
  async (input) => {
    const logger = new Logger()
    logger.info('Creating user', { email: input.email })
    return { id: '123', email: input.email }
  }
)`,
      python: `async def create_user(input):
    logger = Logger()
    logger.info("Creating user", {
        "email": input["email"]
    })
    return {
        "id": "123",
        "email": input["email"]
    }

iii.register_function("users::create", create_user)`,
      rust: `iii.register_function(
    RegisterFunction::new("users::create", create_user)
)`,
    },
  },
  {
    id: 'trigger',
    icon: Zap,
    name: 'Triggers',
    tagline: 'What makes a Function run.',
    description:
      'A Trigger causes a Function to execute — either explicitly from code via trigger(), or automatically from an event source like an HTTP request, cron schedule, queue message, or state change.',
    highlights: [
      'HTTP, cron, queue, subscribe, state, stream triggers',
      'One function, many triggers — bind freely',
      'Custom trigger types plug in at runtime',
      'Same pattern for every event source',
    ],
    code: {
      typescript: `await iii.trigger({ function_id: 'users::create',
                    payload: { name: 'Alice' } });

iii.registerTrigger({
  type: 'http',
  function_id: 'users::create',
  config: {
    api_path: 'users',
    http_method: 'POST',
  },
});

`,
      python: `await iii.trigger({"function_id": "users::create",
                   "payload": {"name": "Alice"}})

iii.register_trigger(
    "http", "users::create",
    {"api_path": "users", "http_method": "POST"}
)`,
      rust: `iii.trigger(TriggerRequest::new("users::create",
            json!({"name": "Alice"}))).await?;

iii.register_trigger(Trigger {
    trigger_type: "http".into(),
    function_id: "users::create".into(),
    config: json!({
        "api_path": "users",
        "http_method": "POST"
    }),
})?;`,
    },
  },
  {
    id: 'worker',
    icon: Share2,
    name: 'Workers',
    tagline: 'Any process that registers functions.',
    description:
      'A Worker is any process that registers Functions and Triggers. Long-running services, ephemeral scripts, agentic workers, or legacy systems via middleware — all connect, register, and communicate seamlessly.',
    highlights: [
      'Workers register functions → immediately available to all',
      'Worker discovery → workers are upgradeable in real time',
      'Long-running, ephemeral, or agentic',
      'Scale up, scale down — topology adapts in real time',
    ],
    code: {
      typescript: `const iii = registerWorker('ws://localhost:49134')`,
      python: `iii = register_worker("ws://localhost:49134")`,

      rust: `let iii = register_worker("ws://localhost:49134", InitOptions::default())?;`,
    },
  },
];

const capabilities = [
  {
    title: 'Unified Invocation',
    description: 'Same interface for all functions.',
    icon: ArrowRight,
    details: [
      'Call any function by string ID — pure address-based routing',
      'Every function uses the same trigger interface',
      'Language-agnostic: TypeScript calls Rust calls Python seamlessly',
    ],
  },
  {
    title: 'Request-Response Correlation',
    description:
      'Sync-style triggers across async boundaries via invocation IDs.',
    icon: Activity,
    details: [
      'Every invocation gets a unique correlation ID',
      'Await results across WebSocket boundaries as if calling a local function',
      'Built-in timeout and retry semantics per invocation',
    ],
  },
  {
    title: 'Lifecycle Management',
    description:
      'Auto-cleanup of functions, triggers, invocations on disconnect.',
    icon: Share2,
    details: [
      'Workers disconnect → their functions and triggers are removed instantly',
      'Clean state, fresh routes — always consistent',
      'Reconnecting workers re-register automatically',
    ],
  },
  {
    title: 'Recursive Orchestration',
    description: 'Engines can nest as workers of other engines.',
    icon: Layers,
    details: [
      'An engine can connect to another engine as a worker',
      'Compose microservice topologies through worker nesting',
      'Scale horizontally by spawning engine sub-clusters',
    ],
  },
];

const capabilityNodes = [
  {
    title: 'HTTP',
    titleFull: 'HTTP + Webhooks',
    subtitle: 'API triggers',
    icon: GlobeIcon,
    tone: 'accent',
    side: 'left',
    type: 'trigger' as const,
  },
  {
    title: 'Cron',
    titleFull: 'Cron + Schedules',
    subtitle: 'Timed execution',
    icon: ClockIcon,
    tone: 'warn',
    side: 'left',
    type: 'trigger' as const,
  },
  {
    title: 'Queues',
    titleFull: 'Queues + Pub/Sub',
    subtitle: 'Messaging',
    icon: MessageCircleIcon,
    tone: 'info',
    side: 'left',
    type: 'trigger' as const,
  },
  {
    title: 'State',
    titleFull: 'State + Cache',
    subtitle: 'Shared context',
    icon: Database,
    tone: 'success',
    side: 'left',
    type: 'trigger' as const,
  },
  {
    title: 'Streaming',
    titleFull: 'Streaming',
    subtitle: 'Realtime pipes',
    icon: Activity,
    tone: 'info',
    side: 'right',
    type: 'function' as const,
  },
  {
    title: 'Traces',
    titleFull: 'Observability',
    subtitle: 'Logs + traces',
    icon: EyeIcon,
    tone: 'accent',
    side: 'right',
    type: 'function' as const,
  },
  {
    title: 'Workflows',
    titleFull: 'Workflows',
    subtitle: 'Orchestration',
    icon: Share2,
    tone: 'warn',
    side: 'right',
    type: 'function' as const,
  },
  {
    title: 'AI Agents',
    titleFull: 'AI Agents',
    subtitle: 'Tool discovery',
    icon: Bot,
    tone: 'alert',
    side: 'right',
    type: 'function' as const,
  },
];

const AccordionItem: React.FC<{
  cap: (typeof capabilities)[0];
  index: number;
  isOpen: boolean;
  onToggle: () => void;
  isDarkMode: boolean;
  textPrimary: string;
  textSecondary: string;
  borderColor: string;
  accentColor: string;
}> = ({
  cap,
  index,
  isOpen,
  onToggle,
  isDarkMode,
  textPrimary,
  textSecondary,
  borderColor,
  accentColor,
}) => {
  const contentRef = useRef<HTMLDivElement>(null);
  const [height, setHeight] = useState(0);
  const Icon = cap.icon;

  useEffect(() => {
    if (contentRef.current) {
      setHeight(isOpen ? contentRef.current.scrollHeight : 0);
    }
  }, [isOpen]);

  const accentHex = isDarkMode ? '#f3f724' : '#2f7fff';

  return (
    <div className={`border-b ${borderColor} transition-colors duration-300`}>
      <button
        onClick={onToggle}
        className={`w-full flex items-center gap-4 md:gap-5 py-4 md:py-6 text-left group transition-all duration-300 ${
          isOpen
            ? ''
            : isDarkMode
              ? 'hover:bg-white/[0.02]'
              : 'hover:bg-black/[0.02]'
        }`}
        aria-expanded={isOpen}
      >
        <div
          className={`w-[2px] h-5 rounded-full transition-all duration-300 flex-shrink-0 mt-2.5`}
          style={{
            backgroundColor: isOpen
              ? accentHex
              : isDarkMode
                ? 'rgba(255,255,255,0.06)'
                : 'rgba(0,0,0,0.06)',
          }}
        />

        <div
          className={`w-10 h-10 rounded-lg flex items-center justify-center flex-shrink-0 transition-all duration-300`}
          style={{
            backgroundColor: isOpen
              ? `${accentHex}15`
              : isDarkMode
                ? 'rgba(255,255,255,0.04)'
                : 'rgba(0,0,0,0.04)',
          }}
        >
          <Icon
            className={`w-4 h-4 transition-colors duration-300 ${
              !isOpen && `${textSecondary} group-hover:${textPrimary}`
            }`}
            style={isOpen ? { color: accentHex } : undefined}
          />
        </div>

        <div className="flex-1 min-w-0">
          <div
            className={`text-base md:text-lg font-bold tracking-tight transition-all duration-300 ${
              isOpen
                ? textPrimary
                : `${textSecondary} group-hover:${textPrimary}`
            }`}
            style={{
              transform: isOpen ? 'translateX(2px)' : undefined,
            }}
          >
            {cap.title}
          </div>
          <div
            className={`text-xs md:text-sm mt-1 transition-colors duration-300 ${
              isOpen
                ? isDarkMode
                  ? 'text-iii-light/60'
                  : 'text-iii-black/60'
                : isDarkMode
                  ? 'text-iii-light/40'
                  : 'text-iii-black/40'
            }`}
          >
            {cap.description}
          </div>
        </div>

        <ChevronDown
          className={`w-5 h-5 flex-shrink-0 transition-all duration-300 ${
            isOpen
              ? `rotate-180`
              : `${isDarkMode ? 'text-iii-light/20' : 'text-iii-black/20'} group-hover:${textSecondary}`
          }`}
          style={isOpen ? { color: accentHex } : undefined}
        />
      </button>

      <div
        className="overflow-hidden transition-all duration-300 ease-[cubic-bezier(0.2,0,0,1)]"
        style={{ height }}
      >
        <div
          ref={contentRef}
          className="pb-6 pl-[60px] md:pl-[76px] pr-4 md:pr-8"
        >
          <div
            className={`rounded-lg p-5 ${
              isDarkMode ? 'bg-[#111]' : 'bg-gray-50'
            }`}
            style={{
              borderLeft: `2px solid ${accentHex}40`,
            }}
          >
            <div className="space-y-3">
              {cap.details.map((detail, i) => (
                <div
                  key={i}
                  className="flex items-start gap-3"
                  style={{
                    animation: isOpen
                      ? `statCount 0.3s ease-out ${i * 0.08}s both`
                      : 'none',
                  }}
                >
                  <div
                    className={`w-1.5 h-1.5 rounded-full mt-1.5 flex-shrink-0`}
                    style={{ backgroundColor: `${accentHex}80` }}
                  />
                  <span
                    className={`text-xs sm:text-sm leading-relaxed ${
                      isDarkMode ? 'text-iii-light/70' : 'text-iii-black/70'
                    }`}
                  >
                    {detail}
                  </span>
                </div>
              ))}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
};

function CapabilitiesAccordion({
  isDarkMode,
  textPrimary,
  textSecondary,
  borderColor,
  accentColor,
}: {
  isDarkMode: boolean;
  textPrimary: string;
  textSecondary: string;
  borderColor: string;
  accentColor: string;
}) {
  const [openIndex, setOpenIndex] = useState<number | null>(null);

  return (
    <div className="mb-12 md:mb-16">
      <div className="text-center mb-8 md:mb-10">
        <h3
          className={`text-xl md:text-2xl lg:text-3xl font-bold ${textPrimary}`}
        >
          Engine capabilities
        </h3>
        <p className={`text-xs md:text-sm mt-2 ${textSecondary}`}>
          Core primitives that compose into any backend pattern
        </p>
      </div>

      <div className={`border-t ${borderColor}`}>
        {capabilities.map((cap, index) => (
          <AccordionItem
            key={index}
            cap={cap}
            index={index}
            isOpen={openIndex === index}
            onToggle={() => setOpenIndex(openIndex === index ? null : index)}
            isDarkMode={isDarkMode}
            textPrimary={textPrimary}
            textSecondary={textSecondary}
            borderColor={borderColor}
            accentColor={accentColor}
          />
        ))}
      </div>
    </div>
  );
}

function ConceptsIDE({
  concepts,
  isDarkMode,
  textPrimary,
  textSecondary,
  borderColor,
  accentColor,
}: {
  concepts: {
    id: string;
    icon: any;
    name: string;
    tagline: string;
    description: string;
    highlights: string[];
    code: Record<string, string>;
  }[];
  isDarkMode: boolean;
  textPrimary: string;
  textSecondary: string;
  borderColor: string;
  accentColor: string;
}) {
  const [activeTab, setActiveTab] = useState(0);
  const [activeLang, setActiveLang] = useState<
    'typescript' | 'python' | 'rust'
  >('typescript');
  const active = concepts[activeTab];
  const Icon = active.icon;

  const accentHex = isDarkMode ? '#f3f724' : '#2f7fff';

  const langs = [
    { key: 'typescript' as const, label: 'TypeScript', short: 'TS' },
    { key: 'python' as const, label: 'Python', short: 'PY' },
    { key: 'rust' as const, label: 'Rust', short: 'RS' },
  ] as const;

  const renderLangToggle = (extraClass: string) => (
    <div
      className={`flex items-center gap-0.5 rounded-full p-1 ${
        isDarkMode ? 'bg-white/[0.06]' : 'bg-black/[0.05]'
      } ${extraClass}`}
    >
      {langs.map((lang) => (
        <button
          key={lang.key}
          onClick={() => setActiveLang(lang.key)}
          className={`flex items-center gap-2 px-3 py-1.5 text-[10px] sm:text-[11px] font-mono rounded-full transition-all duration-200 ${
            activeLang === lang.key
              ? isDarkMode
                ? 'bg-white/[0.12] text-white'
                : 'bg-white text-black shadow-sm'
              : isDarkMode
                ? 'text-white/40 hover:text-white/60'
                : 'text-black/40 hover:text-black/60'
          }`}
        >
          <LangIcon lang={lang.key} active={activeLang === lang.key} />
          <span className="hidden sm:inline">{lang.label}</span>
          <span className="sm:hidden">{lang.short}</span>
        </button>
      ))}
    </div>
  );

  return (
    <div className="mb-16 md:mb-24">
      {/* Single IDE Window */}
      <div
        className={`rounded-lg border overflow-hidden ${borderColor} ${isDarkMode ? 'bg-[#0c0c0c]' : 'bg-white'}`}
        style={{ boxShadow: `0 0 40px ${accentHex}08` }}
      >
        {/* Title bar with macOS dots + file tabs */}
        <div
          className={`flex items-center border-b ${borderColor} ${isDarkMode ? 'bg-[#111]' : 'bg-gray-50'}`}
        >
          {/* macOS dots */}
          <div className="flex items-center gap-1.5 px-4 py-3 flex-shrink-0">
            <div className="w-2.5 h-2.5 rounded-full bg-[#ff5f57]" />
            <div className="w-2.5 h-2.5 rounded-full bg-[#febc2e]" />
            <div className="w-2.5 h-2.5 rounded-full bg-[#28c840]" />
          </div>

          {/* File tabs */}
          <div className="flex -mb-px overflow-x-auto">
            {concepts.map((concept: any, i: number) => {
              const TabIcon = concept.icon;
              const isActive = activeTab === i;
              return (
                <button
                  key={concept.id}
                  onClick={() => setActiveTab(i)}
                  className={`flex items-center gap-2 px-4 py-2.5 text-xs font-mono border-b-2 transition-all duration-200 whitespace-nowrap ${
                    isActive
                      ? `${isDarkMode ? 'text-white bg-[#1a1a1a]' : 'text-black bg-white'}`
                      : `${isDarkMode ? 'text-white/40 hover:text-white/60 hover:bg-white/[0.02]' : 'text-black/40 hover:text-black/60 hover:bg-black/[0.02]'}`
                  }`}
                  style={{
                    borderBottomColor: isActive ? accentHex : 'transparent',
                  }}
                >
                  <TabIcon
                    className={`w-3.5 h-3.5 ${isActive ? accentColor : ''}`}
                  />
                  {concept.name}
                </button>
              );
            })}
          </div>

          <div className="flex-1" />

          {renderLangToggle('hidden md:flex mr-4')}
        </div>

        {/* Content: Description left + Code right */}
        <div className="grid grid-cols-1 lg:grid-cols-[1.2fr_1fr] lg:min-h-[340px]">
          {/* Left: Description panel */}
          <div
            className={`p-6 sm:p-8 border-b lg:border-b-0 lg:border-r flex flex-col justify-center ${borderColor}`}
          >
            <div className="flex items-center gap-3 mb-4">
              <div
                className="p-2 rounded-lg"
                style={{ backgroundColor: `${accentHex}15` }}
              >
                <Icon size={20} style={{ color: accentHex }} />
              </div>
              <div>
                <h3 className={`text-lg font-bold ${textPrimary}`}>
                  {active.name}
                </h3>
                <p className="text-xs font-mono" style={{ color: accentHex }}>
                  {active.tagline}
                </p>
              </div>
            </div>

            <p className={`text-sm leading-relaxed mb-6 ${textSecondary}`}>
              {active.description}
            </p>

            <div className="space-y-3">
              {active.highlights.map((h: string, i: number) => (
                <div key={i} className="flex items-start gap-2.5">
                  <div
                    className="w-1.5 h-1.5 rounded-full mt-1.5 flex-shrink-0"
                    style={{ backgroundColor: `${accentHex}80` }}
                  />
                  <span
                    className={`text-xs leading-relaxed ${isDarkMode ? 'text-white/65' : 'text-black/65'}`}
                  >
                    {h}
                  </span>
                </div>
              ))}
            </div>
          </div>

          {/* Right: Code panel */}
          <div
            className={`relative flex flex-col overflow-hidden ${isDarkMode ? 'bg-[#0a0a0a]' : 'bg-[#fafafa]'}`}
          >
            {/* Mobile language toggle — inside code panel */}
            <div
              className={`flex md:hidden justify-center py-3 border-b ${borderColor}`}
            >
              {renderLangToggle('')}
            </div>
            {/* Code Block with Syntax Highlighting */}
            <div className="flex flex-1">
              <Highlight
                theme={isDarkMode ? themes.vsDark : themes.vsLight}
                code={active.code[activeLang]}
                language={
                  activeLang === 'rust'
                    ? 'rust'
                    : activeLang === 'python'
                      ? 'python'
                      : 'typescript'
                }
              >
                {({
                  className,
                  style,
                  tokens,
                  getLineProps,
                  getTokenProps,
                }) => (
                  <>
                    {/* Line numbers */}
                    <div
                      className={`flex-shrink-0 pt-6 pb-6 pl-4 pr-3 text-right select-none ${isDarkMode ? 'text-white/20' : 'text-black/20'}`}
                      style={{ paddingTop: '2.5rem' }}
                    >
                      {tokens.map((_, i) => (
                        <div
                          key={i}
                          className="text-xs sm:text-sm leading-[1.7] font-mono"
                        >
                          {i + 1}
                        </div>
                      ))}
                    </div>
                    {/* Code */}
                    <pre
                      className={`flex-1 p-6 pl-0 overflow-x-auto text-xs sm:text-sm leading-[1.7] font-mono`}
                      style={{
                        ...style,
                        backgroundColor: 'transparent',
                        paddingTop: '2.5rem',
                      }}
                    >
                      {tokens.map((line, i) => (
                        <div key={i} {...getLineProps({ line, key: i })}>
                          {line.map((token, key) => (
                            <span
                              key={key}
                              {...getTokenProps({ token, key })}
                            />
                          ))}
                        </div>
                      ))}
                    </pre>
                  </>
                )}
              </Highlight>
            </div>

            {/* Status bar */}
          </div>
        </div>
      </div>
    </div>
  );
}

export function EngineSection({ isDarkMode = true }: EngineSectionProps) {
  const [activeNode, setActiveNode] = useState<string | null>(null);
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

  const toneClasses = {
    accent: {
      icon: isDarkMode ? 'text-iii-accent' : 'text-iii-accent-light',
      bg: isDarkMode ? 'bg-iii-accent/10' : 'bg-iii-accent-light/10',
      border: isDarkMode
        ? 'border-iii-accent/20'
        : 'border-iii-accent-light/20',
    },
    info: {
      icon: 'text-iii-info',
      bg: 'bg-iii-info/10',
      border: 'border-iii-info/20',
    },
    warn: {
      icon: 'text-iii-warn',
      bg: 'bg-iii-warn/10',
      border: 'border-iii-warn/20',
    },
    success: {
      icon: 'text-iii-success',
      bg: 'bg-iii-success/10',
      border: 'border-iii-success/20',
    },
    alert: {
      icon: 'text-iii-alert',
      bg: 'bg-iii-alert/10',
      border: 'border-iii-alert/20',
    },
  } as const;

  const leftNodes = capabilityNodes.filter((node) => node.side === 'left');
  const rightNodes = capabilityNodes.filter((node) => node.side === 'right');

  return (
    <section
      className={`relative w-full overflow-hidden font-mono transition-colors duration-300 ${textPrimary}`}
    >
      {/* Subtle ambient glow decoration */}
      <div className="absolute inset-0 pointer-events-none overflow-hidden">
        <div
          className="absolute top-1/4 -right-1/4 w-1/2 h-1/2 rounded-full opacity-[0.02]"
          style={{
            background:
              'radial-gradient(circle, var(--color-accent) 0%, transparent 70%)',
          }}
        />
      </div>
      <div className="relative z-10 w-full max-w-7xl mx-auto px-4 sm:px-6 py-8 md:py-10">
        {/* Header — PlanetScale-inspired: big statement, tight subtitle */}
        <div className="text-center mb-10 md:mb-16 space-y-5">
          <h2 className="text-2xl sm:text-4xl md:text-5xl lg:text-6xl font-bold tracking-tighter leading-[1.05]">
            <span className="block">One engine.</span>
            <span className={`block ${accentColor}`}>Three primitives.</span>
          </h2>
          <p
            className={`text-sm md:text-base lg:text-lg max-w-2xl mx-auto leading-relaxed ${textSecondary}`}
          >
            It really is as simple as Functions, Triggers, and Workers
          </p>
        </div>

        {/* Concepts — Tabbed IDE Component */}
        <ConceptsIDE
          concepts={concepts}
          isDarkMode={isDarkMode}
          textPrimary={textPrimary}
          textSecondary={textSecondary}
          borderColor={borderColor}
          accentColor={accentColor}
        />

        {/* Capability Map Diagram — commented out
        <div
          className={`relative p-4 sm:p-6 md:p-8 lg:p-10 rounded-lg ${bgCard} overflow-hidden`}
        >
          <div
            className="absolute inset-0 opacity-[0.03] pointer-events-none"
            style={{
              backgroundImage: `
                linear-gradient(${isDarkMode ? 'var(--color-light)' : 'var(--color-black)'} 1px, transparent 1px),
                linear-gradient(90deg, ${isDarkMode ? 'var(--color-light)' : 'var(--color-black)'} 1px, transparent 1px)
              `,
              backgroundSize: '40px 40px',
            }}
          />

          <div className="relative z-10">
            <div className="text-center mb-6 md:mb-10">
              <h3
                className={`text-xl md:text-2xl font-mono font-bold tracking-tight ${textPrimary}`}
              >
                The Engine does it all
              </h3>
              <p className={`text-sm mt-3 font-mono ${textSecondary}`}>
                Operations flow through the Engine to any worker, in any
                language
              </p>
            </div>

            <div className="lg:hidden flex flex-col items-center gap-0">
              <div className="grid grid-cols-2 gap-2 sm:gap-3 w-full max-w-md">
                {leftNodes.map((node: any) => {
                  const Icon = node.icon;
                  const tone =
                    toneClasses[node.tone as keyof typeof toneClasses];
                  return (
                    <div
                      key={node.title}
                      className={`rounded-lg border ${borderColor} ${bgCard} px-2.5 sm:px-3 py-2 sm:py-2.5 flex items-center gap-2`}
                    >
                      <div
                        className={`flex h-7 w-7 sm:h-8 sm:w-8 items-center justify-center rounded-lg border overflow-hidden ${tone.border} ${tone.bg} flex-shrink-0`}
                      >
                        <Icon className={`${tone.icon}`} size={14} />
                      </div>
                      <div className="text-left min-w-0">
                        <div className="flex items-center gap-1.5">
                          <div
                            className={`text-[10px] sm:text-xs font-semibold ${textPrimary} leading-tight`}
                          >
                            <span className="sm:hidden">{node.title}</span>
                            <span className="hidden sm:inline">
                              {node.titleFull}
                            </span>
                          </div>
                          <span
                            className={`text-[8px] font-mono font-bold px-1 py-0.5 rounded border leading-none ${
                              node.type === 'trigger'
                                ? isDarkMode
                                  ? 'border-iii-accent/40 text-iii-accent/80'
                                  : 'border-iii-accent-light/40 text-iii-accent-light/80'
                                : isDarkMode
                                  ? 'border-iii-info/40 text-iii-info/80'
                                  : 'border-iii-info/40 text-iii-info/80'
                            }`}
                          >
                            {node.type === 'trigger' ? 'T' : 'F'}
                          </span>
                        </div>
                        <div
                          className={`text-[9px] sm:text-[10px] ${textSecondary} leading-tight`}
                        >
                          {node.subtitle}
                        </div>
                      </div>
                    </div>
                  );
                })}
              </div>

              <div className="flex flex-col items-center py-1">
                <AnimatedConnector
                  isDarkMode={isDarkMode}
                  orientation="vertical"
                  length={28}
                  duration="1.4s"
                  className="overflow-visible"
                />
              </div>

              <div className="relative flex items-center justify-center w-full max-w-xs">
                <div
                  className={`relative z-10 w-full flex flex-col items-center justify-center px-6 py-6 rounded-lg border-2 ${isDarkMode ? 'bg-[#0a0a0a]' : 'bg-white'}`}
                  style={{
                    borderColor: isDarkMode ? '#f3f724' : '#2f7fff',
                    boxShadow: `0 0 25px ${isDarkMode ? '#f3f724' : '#2f7fff'}30`,
                  }}
                >
                  <Logo
                    className={`h-3 mb-1 ${isDarkMode ? 'text-white/50' : 'text-black/50'}`}
                  />
                  <div
                    className="text-xl font-bold font-mono tracking-tight"
                    style={{ color: isDarkMode ? '#f3f724' : '#2f7fff' }}
                  >
                    Engine
                  </div>
                  <div
                    className={`mt-2 px-3 py-1 rounded-full border text-[10px] font-mono flex items-center gap-1.5 ${
                      isDarkMode
                        ? 'bg-white/5 border-white/10 text-white/70'
                        : 'bg-black/5 border-black/10 text-black/70'
                    }`}
                  >
                    <Activity
                      className="w-3 h-3"
                      style={{ color: isDarkMode ? '#f3f724' : '#2f7fff' }}
                    />
                    <span>
                      12,869 <span className="opacity-50">ops</span>
                    </span>
                  </div>
                  <div
                    className={`mt-2 text-[9px] font-mono text-center ${isDarkMode ? 'text-white/40' : 'text-black/40'}`}
                  >
                    Triggers &bull; Functions &bull; Workers
                  </div>
                </div>
              </div>

              <div className="flex flex-col items-center py-1">
                <AnimatedConnector
                  isDarkMode={isDarkMode}
                  orientation="vertical"
                  length={28}
                  duration="1.4s"
                  className="overflow-visible"
                />
              </div>

              <div className="grid grid-cols-2 gap-2 sm:gap-3 w-full max-w-md">
                {rightNodes.map((node: any) => {
                  const Icon = node.icon;
                  const tone =
                    toneClasses[node.tone as keyof typeof toneClasses];
                  return (
                    <div
                      key={node.title}
                      className={`rounded-lg border ${borderColor} ${bgCard} px-2.5 sm:px-3 py-2 sm:py-2.5 flex items-center gap-2`}
                    >
                      <div
                        className={`flex h-7 w-7 sm:h-8 sm:w-8 items-center justify-center rounded-lg border overflow-hidden ${tone.border} ${tone.bg} flex-shrink-0`}
                      >
                        <Icon className={`${tone.icon}`} size={14} />
                      </div>
                      <div className="text-left min-w-0">
                        <div className="flex items-center gap-1.5">
                          <div
                            className={`text-[10px] sm:text-xs font-semibold ${textPrimary} leading-tight`}
                          >
                            <span className="sm:hidden">{node.title}</span>
                            <span className="hidden sm:inline">
                              {node.titleFull}
                            </span>
                          </div>
                          <span
                            className={`text-[8px] font-mono font-bold px-1 py-0.5 rounded border leading-none ${
                              node.type === 'trigger'
                                ? isDarkMode
                                  ? 'border-iii-accent/40 text-iii-accent/80'
                                  : 'border-iii-accent-light/40 text-iii-accent-light/80'
                                : isDarkMode
                                  ? 'border-iii-info/40 text-iii-info/80'
                                  : 'border-iii-info/40 text-iii-info/80'
                            }`}
                          >
                            {node.type === 'trigger' ? 'T' : 'F'}
                          </span>
                        </div>
                        <div
                          className={`text-[9px] sm:text-[10px] ${textSecondary} leading-tight`}
                        >
                          {node.subtitle}
                        </div>
                      </div>
                    </div>
                  );
                })}
              </div>
            </div>

            <div className="hidden lg:block w-full mx-auto">
              <ParticleFlowDiagram
                leftNodes={leftNodes}
                rightNodes={rightNodes}
                isDarkMode={isDarkMode}
                toneClasses={toneClasses}
              />
            </div>
          </div>
        </div>
        */}
      </div>
    </section>
  );
}

// --- Particle Flow Engine Subcomponent ---

function ParticleFlowDiagram({
  leftNodes,
  rightNodes,
  isDarkMode,
  toneClasses,
}: any) {
  // Cycle through inputs automatically: 0, 1, 2, 3
  const [activeInput, setActiveInput] = useState(0);
  const [cycleKey, setCycleKey] = useState(0);
  const [eventCount, setEventCount] = useState(12847);
  const [isHovered, setIsHovered] = useState(false);
  const [hoveredNode, setHoveredNode] = useState<string | null>(null);

  // Auto-cycle every 3.5s if not hovered
  useEffect(() => {
    if (isHovered && hoveredNode !== null) return;

    const interval = setInterval(() => {
      setActiveInput((prev) => (prev + 1) % 4);
      setCycleKey((prev) => prev + 1);
    }, 2000);
    return () => clearInterval(interval);
  }, [isHovered, hoveredNode]);

  // Increment counter halfway through the cycle (when particles hit the engine)
  useEffect(() => {
    if (isHovered && hoveredNode !== null) return;

    const timer = setTimeout(() => {
      setEventCount((prev) => prev + Math.floor(Math.random() * 3) + 1);
    }, 1000);
    return () => clearTimeout(timer);
  }, [cycleKey, isHovered, hoveredNode]);

  // Manual hover handling
  const handleNodeHover = (
    side: 'left' | 'right',
    index: number,
    title: string,
  ) => {
    setIsHovered(true);
    setHoveredNode(title);
    if (side === 'left') {
      if (activeInput !== index) {
        setActiveInput(index);
        setCycleKey((prev) => prev + 1); // trigger animation immediately
        setTimeout(() => setEventCount((e) => e + 1), 1000);
      }
    }
  };

  const handleNodeLeave = () => {
    setIsHovered(false);
    setHoveredNode(null);
  };

  // Dimensions
  const w = 900;
  const h = 420;

  // Node layout coordinates
  const leftX = 0;
  const nodeW = 200;
  const nodeH = 56;
  const gapY = 32;
  const rightX = w - nodeW;

  // Y positions for the 4 nodes on each side
  const yPos = [
    (h - (4 * nodeH + 3 * gapY)) / 2, // 34
    (h - (4 * nodeH + 3 * gapY)) / 2 + nodeH + gapY, // 122
    (h - (4 * nodeH + 3 * gapY)) / 2 + 2 * (nodeH + gapY), // 210
    (h - (4 * nodeH + 3 * gapY)) / 2 + 3 * (nodeH + gapY), // 298
  ];

  // Engine center
  const cx = w / 2;
  const cy = h / 2;

  // Bezier paths
  // Left nodes to engine (anchor at right edge of left node)
  const leftPaths = yPos.map((y) => {
    const startX = leftX + nodeW;
    const startY = y + nodeH / 2;
    return `M ${startX},${startY} C ${startX + 60},${startY} ${cx - 100},${cy} ${cx - 50},${cy}`;
  });

  // Engine to right nodes (anchor at left edge of right node)
  const rightPaths = yPos.map((y) => {
    const endX = rightX;
    const endY = y + nodeH / 2;
    return `M ${cx + 50},${cy} C ${cx + 100},${cy} ${endX - 60},${endY} ${endX},${endY}`;
  });

  // Reverse paths for bidirectional flow
  const leftPathsReverse = yPos.map((y) => {
    const startX = leftX + nodeW;
    const startY = y + nodeH / 2;
    return `M ${cx - 50},${cy} C ${cx - 100},${cy} ${startX + 60},${startY} ${startX},${startY}`;
  });

  const rightPathsReverse = yPos.map((y) => {
    const endX = rightX;
    const endY = y + nodeH / 2;
    return `M ${endX},${endY} C ${endX - 60},${endY} ${cx + 100},${cy} ${cx + 50},${cy}`;
  });

  const getThemeColor = (colorName: string) => {
    return isDarkMode ? '#f3f724' : '#2f7fff';
  };

  const activeColor = getThemeColor(
    leftNodes[activeInput].tone.split('-')[0] || 'purple',
  );
  const baseLineColor = isDarkMode
    ? 'rgba(255,255,255,0.08)'
    : 'rgba(0,0,0,0.06)';

  return (
    <svg
      viewBox={`0 0 ${w} ${h}`}
      className="w-full h-auto overflow-visible font-sans"
    >
      <defs>
        {/* Glow filters */}
        <filter id="glow" x="-20%" y="-20%" width="140%" height="140%">
          <feGaussianBlur stdDeviation="4" result="blur" />
          <feComposite in="SourceGraphic" in2="blur" operator="over" />
        </filter>
        <filter id="glow-large" x="-50%" y="-50%" width="200%" height="200%">
          <feGaussianBlur stdDeviation="12" result="blur" />
          <feComposite in="SourceGraphic" in2="blur" operator="over" />
        </filter>
      </defs>

      {/* BACKGROUND PATHS */}
      {leftPaths.map((d, i) => (
        <path
          key={`bg-l-${i}`}
          d={d}
          fill="none"
          stroke={baseLineColor}
          strokeWidth="1.5"
        />
      ))}
      {rightPaths.map((d, i) => (
        <path
          key={`bg-r-${i}`}
          d={d}
          fill="none"
          stroke={baseLineColor}
          strokeWidth="1.5"
        />
      ))}

      {/* AMBIENT PARTICLES (denser bidirectional stream) */}
      {[0, 1, 2, 3].map((i) =>
        [0, 1, 2].map((j) => (
          <g key={`ambient-fwd-${i}-${j}`} opacity="0.3">
            <circle r="1.5" fill={isDarkMode ? '#ffffff' : '#000000'}>
              <animateMotion
                dur="2.4s"
                begin={`${i * 0.45 + j * 0.52}s`}
                repeatCount="indefinite"
                path={leftPaths[i]}
              />
            </circle>
            <circle r="1.5" fill={isDarkMode ? '#ffffff' : '#000000'}>
              <animateMotion
                dur="2.4s"
                begin={`${i * 0.45 + j * 0.52 + 1.1}s`}
                repeatCount="indefinite"
                path={rightPaths[i]}
              />
            </circle>
          </g>
        )),
      )}
      {[0, 1, 2, 3].map((i) =>
        [0, 1].map((j) => (
          <g key={`ambient-rev-${i}-${j}`} opacity="0.2">
            <circle r="1.5" fill={isDarkMode ? '#ffffff' : '#000000'}>
              <animateMotion
                dur="2.8s"
                begin={`${i * 0.4 + j * 1.05}s`}
                repeatCount="indefinite"
                path={leftPathsReverse[i]}
              />
            </circle>
            <circle r="1.5" fill={isDarkMode ? '#ffffff' : '#000000'}>
              <animateMotion
                dur="2.8s"
                begin={`${i * 0.4 + j * 1.05 + 0.45}s`}
                repeatCount="indefinite"
                path={rightPathsReverse[i]}
              />
            </circle>
          </g>
        )),
      )}
      {[1, 3].map((i) => (
        <g key={`ambient-rev-single-${i}`} opacity="0.2">
          <circle r="1.5" fill={isDarkMode ? '#ffffff' : '#000000'}>
            <animateMotion
              dur="3s"
              begin={`${i * 0.9}s`}
              repeatCount="indefinite"
              path={leftPathsReverse[i]}
            />
          </circle>
          <circle r="1.5" fill={isDarkMode ? '#ffffff' : '#000000'}>
            <animateMotion
              dur="3s"
              begin={`${i * 0.9 + 1}s`}
              repeatCount="indefinite"
              path={rightPathsReverse[i]}
            />
          </circle>
        </g>
      ))}

      {/* ACTIVE HIGHLIGHT PATH */}
      {/* Left side: only the active input path glows */}
      <path
        d={leftPaths[activeInput]}
        fill="none"
        stroke={activeColor}
        strokeWidth="2"
        opacity="0.6"
        filter="url(#glow)"
        style={{ transition: 'stroke 0.3s' }}
      />
      {/* Right side: all paths glow when outputting */}
      {rightPaths.map((d, i) => (
        <g key={`active-r-${i}-${cycleKey}`}>
          <path
            d={d}
            fill="none"
            stroke={activeColor}
            strokeWidth="2"
            opacity="0"
            filter="url(#glow)"
          >
            {/* Delay glow until particles reach center */}
            <animate
              attributeName="opacity"
              values="0;0;0.5;0"
              keyTimes="0;0.35;0.5;1"
              dur="1.8s"
              begin="0s"
            />
          </path>
        </g>
      ))}

      {/* MAIN ANIMATED PARTICLES (Triggered on cycleKey change) */}
      {/* 1. Input → Engine (5 staggered particles) */}
      {[0, 0.08, 0.16, 0.24, 0.32].map((delay, j) => (
        <circle
          key={`p-in-${cycleKey}-${j}`}
          r="3.5"
          fill={activeColor}
          filter="url(#glow)"
          opacity="0"
        >
          <animate
            attributeName="opacity"
            values="0;1;1;0"
            keyTimes="0;0.05;0.95;1"
            dur="0.6s"
            begin={`${delay}s`}
            fill="freeze"
          />
          <animateMotion
            dur="0.6s"
            begin={`${delay}s`}
            fill="freeze"
            path={leftPaths[activeInput]}
          />
        </circle>
      ))}

      {/* 2. Engine → Outputs (Fan out to all 4 simultaneously, 5 particles each) */}
      {[0, 1, 2, 3].map((outputIndex) => (
        <g key={`p-out-${cycleKey}-${outputIndex}`}>
          {[0.65, 0.73, 0.81, 0.89, 0.97].map((delay, j) => (
            <circle
              key={`p-out-c-${j}`}
              r="3.5"
              fill={activeColor}
              filter="url(#glow)"
              opacity="0"
            >
              <animate
                attributeName="opacity"
                values="0;1;1;0"
                keyTimes="0;0.05;0.95;1"
                dur="0.6s"
                begin={`${delay}s`}
                fill="freeze"
              />
              <animateMotion
                dur="0.6s"
                begin={`${delay}s`}
                fill="freeze"
                path={rightPaths[outputIndex]}
              />
            </circle>
          ))}
        </g>
      ))}

      {/* LEFT NODES (Inputs) */}
      {leftNodes.map((node: any, i: number) => {
        const isActive = activeInput === i;
        const isMuted =
          !isActive && hoveredNode !== null && hoveredNode !== node.title;
        const tone = toneClasses[node.tone as keyof typeof toneClasses];
        const Icon = node.icon;

        return (
          <foreignObject
            key={`ln-${i}`}
            x={leftX}
            y={yPos[i]}
            width={nodeW}
            height={nodeH}
            onMouseEnter={() => handleNodeHover('left', i, node.title)}
            onMouseLeave={handleNodeLeave}
          >
            <div
              className={`w-full h-full rounded-lg flex items-center gap-3 px-3 transition-all duration-300 ${
                isActive
                  ? 'border-2 border-solid shadow-[0_0_15px_rgba(0,0,0,0.2)] scale-[1.02]'
                  : `border border-solid ${isDarkMode ? 'border-white/10 bg-black/40' : 'border-black/10 bg-white/40'} ${isMuted ? 'opacity-40' : 'hover:border-iii-medium/30'}`
              }`}
              style={
                isActive
                  ? {
                      borderColor: activeColor,
                      backgroundColor: isDarkMode
                        ? 'rgba(0,0,0,0.6)'
                        : 'rgba(255,255,255,0.8)',
                    }
                  : {}
              }
            >
              <div
                className={`flex h-8 w-8 items-center justify-center rounded-lg border overflow-hidden flex-shrink-0 transition-colors ${
                  isActive
                    ? `${tone.border} ${tone.bg}`
                    : `${isDarkMode ? 'border-white/10' : 'border-black/10'} ${tone.bg}`
                }`}
              >
                <Icon size={16} className={tone.icon} />
              </div>
              <div className="text-left min-w-0">
                <div className="flex items-center gap-1.5">
                  <div
                    className={`text-xs font-semibold truncate transition-colors ${
                      isActive
                        ? isDarkMode
                          ? 'text-white'
                          : 'text-black'
                        : isDarkMode
                          ? 'text-white/70'
                          : 'text-black/70'
                    }`}
                  >
                    {node.titleFull}
                  </div>
                  <span
                    className={`text-[8px] font-mono font-bold px-1 py-0.5 rounded border leading-none flex-shrink-0 ${
                      node.type === 'trigger'
                        ? isDarkMode
                          ? 'border-[#f3f724]/40 text-[#f3f724]/80'
                          : 'border-[#2f7fff]/40 text-[#2f7fff]/80'
                        : isDarkMode
                          ? 'border-[#38bdf8]/40 text-[#38bdf8]/80'
                          : 'border-[#38bdf8]/40 text-[#38bdf8]/80'
                    }`}
                  >
                    {node.type === 'trigger' ? 'T' : 'F'}
                  </span>
                </div>
                <div
                  className={`text-[10px] truncate ${isDarkMode ? 'text-white/40' : 'text-black/40'}`}
                >
                  {node.subtitle}
                </div>
              </div>
            </div>
          </foreignObject>
        );
      })}

      {/* RIGHT NODES (Outputs) */}
      {rightNodes.map((node: any, i: number) => {
        // Output nodes highlight briefly when particles arrive
        const isOutputActive = !isHovered || hoveredNode === node.title;
        const isMuted = isHovered && hoveredNode !== node.title;
        const tone = toneClasses[node.tone as keyof typeof toneClasses];
        const Icon = node.icon;

        return (
          <foreignObject
            key={`rn-${i}`}
            x={rightX}
            y={yPos[i]}
            width={nodeW}
            height={nodeH}
            onMouseEnter={() => handleNodeHover('right', i, node.title)}
            onMouseLeave={handleNodeLeave}
          >
            <div className="relative w-full h-full">
              {/* Pulse effect triggered by cycleKey */}
              <div
                key={`pulse-${cycleKey}`}
                className="absolute inset-0 rounded-lg bg-current opacity-0 pointer-events-none"
                style={{
                  color: activeColor,
                  animation: `pulse-fade 1s ease-out 2s`,
                }}
              />
              <style>{`
                @keyframes pulse-fade {
                  0% { opacity: 0; transform: scale(0.95); }
                  20% { opacity: 0.15; transform: scale(1.02); }
                  100% { opacity: 0; transform: scale(1); }
                }
              `}</style>

              <div
                className={`relative w-full h-full rounded-lg border flex items-center gap-3 px-3 transition-all duration-300 ${
                  isMuted
                    ? `${isDarkMode ? 'border-white/10 bg-black/40' : 'border-black/10 bg-white/40'} opacity-40`
                    : `${isDarkMode ? 'border-white/10 bg-black/40' : 'border-black/10 bg-white/40'}`
                }`}
              >
                <div
                  className={`flex h-8 w-8 items-center justify-center rounded-lg border overflow-hidden flex-shrink-0 ${isDarkMode ? 'border-white/10' : 'border-black/10'} ${tone.bg}`}
                >
                  <Icon size={16} className={tone.icon} />
                </div>
                <div className="text-left min-w-0">
                  <div className="flex items-center gap-1.5">
                    <div
                      className={`text-xs font-semibold truncate ${isDarkMode ? 'text-white/80' : 'text-black/80'}`}
                    >
                      {node.titleFull}
                    </div>
                    <span
                      className={`text-[8px] font-mono font-bold px-1 py-0.5 rounded border leading-none flex-shrink-0 ${
                        node.type === 'trigger'
                          ? isDarkMode
                            ? 'border-[#f3f724]/40 text-[#f3f724]/80'
                            : 'border-[#2f7fff]/40 text-[#2f7fff]/80'
                          : isDarkMode
                            ? 'border-[#38bdf8]/40 text-[#38bdf8]/80'
                            : 'border-[#38bdf8]/40 text-[#38bdf8]/80'
                      }`}
                    >
                      {node.type === 'trigger' ? 'T' : 'F'}
                    </span>
                  </div>
                  <div
                    className={`text-[10px] truncate ${isDarkMode ? 'text-white/40' : 'text-black/40'}`}
                  >
                    {node.subtitle}
                  </div>
                </div>
              </div>
            </div>
          </foreignObject>
        );
      })}

      {/* CENTRAL ENGINE HUB */}
      <g transform={`translate(${cx}, ${cy})`}>
        {/* Breathing ring */}
        <circle
          r="65"
          fill="none"
          stroke={activeColor}
          strokeWidth="1"
          opacity="0.2"
        >
          <animate
            attributeName="r"
            values="60;70;60"
            dur="4s"
            repeatCount="indefinite"
          />
          <animate
            attributeName="opacity"
            values="0.1;0.3;0.1"
            dur="4s"
            repeatCount="indefinite"
          />
        </circle>

        {/* Pulse effect on impact */}
        <circle
          key={`engine-pulse-${cycleKey}`}
          r="55"
          fill="none"
          stroke={activeColor}
          strokeWidth="2"
          opacity="0"
        >
          <animate
            attributeName="r"
            values="55;85"
            dur="1s"
            begin="0.8s"
            fill="freeze"
          />
          <animate
            attributeName="opacity"
            values="0.5;0"
            dur="1s"
            begin="0.8s"
            fill="freeze"
          />
        </circle>

        {/* Engine Container (using foreignObject for Tailwind styling) */}
        <foreignObject x="-90" y="-80" width="180" height="160">
          <div
            className={`w-full h-full flex flex-col items-center justify-center rounded-lg border-2 transition-colors duration-500 ${
              isDarkMode ? 'bg-[#0a0a0a]' : 'bg-white'
            }`}
            style={{
              borderColor: activeColor,
              boxShadow: `0 0 30px ${activeColor}40`,
            }}
          >
            <Logo
              className={`h-3 mb-2 transition-colors duration-500 ${isDarkMode ? 'text-white/50' : 'text-black/50'}`}
            />
            <div
              className="text-2xl font-bold font-mono tracking-tight transition-colors duration-500"
              style={{ color: activeColor }}
            >
              Engine
            </div>

            {/* Live Counter */}
            <div
              className={`mt-3 px-3 py-1 rounded-full border text-[10px] font-mono flex items-center gap-1.5 transition-colors duration-500 ${
                isDarkMode
                  ? 'bg-white/5 border-white/10 text-white/70'
                  : 'bg-black/5 border-black/10 text-black/70'
              }`}
            >
              <Activity className="w-3 h-3" style={{ color: activeColor }} />
              <span>
                {eventCount.toLocaleString()}{' '}
                <span className="opacity-50">ops</span>
              </span>
            </div>

            <div
              className={`mt-2 text-[9px] font-mono text-center px-4 ${isDarkMode ? 'text-white/40' : 'text-black/40'}`}
            >
              Triggers &bull; Functions
              <br />
              Workers
            </div>
          </div>
        </foreignObject>
      </g>
    </svg>
  );
}
