import React, { useState, useEffect, useRef, useCallback } from 'react';
import { Highlight, themes } from 'prism-react-renderer';

type AnimationPhase =
  | 'idle'
  | 'highlighting'
  | 'moving'
  | 'linking'
  | 'connected'
  | 'outputting'
  | 'legendVisible' // Legend appears first
  | 'spotlight'; // Code highlighting animates in

// Keywords/patterns that indicate architecture/infrastructure code
// Covers: JS/TS, Python, Rust
const architectureKeywords = [
  // Imports & modules
  'import',
  'require',
  'from',
  'use ',
  'mod ',
  'extern crate',
  'include',

  // Environment & config
  'process.env',
  'os.environ',
  'std::env',
  'dotenv',
  'env::',
  'env.',
  'getenv',
  'config',
  'settings',
  'options',
  '.env',

  // Connection & initialization
  'connect',
  'connection',
  'initialize',
  'init',
  'setup',
  'bootstrap',
  'createclient',
  'createconnection',
  'createpool',
  'createadapter',
  'client(',
  'pool(',
  'getconnection',

  // Message queues & brokers
  'redis',
  'bull',
  'queue',
  'kafka',
  'rabbit',
  'amqp',
  'celery',
  'rq',
  'pubsub',
  'nats',
  'zeromq',
  'zmq',
  'sqs',
  'sns',
  'eventbridge',

  // Real-time & sockets
  'socket',
  'websocket',
  'io(',
  'pusher',
  'ably',
  'centrifugo',
  'subscribe',
  'unsubscribe',
  'on(',
  'once(',
  'addeventlistener',
  'removelistener',
  'listener',
  'handler',
  'middleware',
  'interceptor',

  // Scheduling & workflows
  'temporal',
  'cron',
  'agenda',
  'schedule',
  'scheduler',
  'celerybeat',
  'airflow',
  'dag',
  'workflow',
  'step function',

  // Logging & tracing
  'winston',
  'pino',
  'logger',
  'logging',
  'log.',
  'tracer',
  'tracing',
  'opentelemetry',
  'datadog',
  'sentry',
  'newrelic',
  'span',

  // Database clients
  'prisma',
  'sequelize',
  'typeorm',
  'mongoose',
  'sqlalchemy',
  'diesel',
  'pg.',
  'mysql',
  'mongodb',
  'dynamodb',
  'ioredis',
  'knex',

  // HTTP/Server setup
  'express',
  'fastify',
  'koa',
  'flask',
  'django',
  'actix',
  'axum',
  'rocket',
  'app.use',
  'app.get',
  'app.post',
  'router.',
  'listen(',
  'bind(',
  'createserver',
  'httpserver',

  // Auth & security
  'passport',
  'jwt',
  'oauth',
  'auth0',
  'cognito',
  'firebase.auth',
  'bcrypt',
  'argon',
  'crypto',

  // Cloud & infrastructure
  'aws.',
  's3.',
  'lambda',
  'cloudformation',
  'terraform',
  'docker',
  'kubernetes',
  'k8s',

  // Error handling boilerplate (when it's setup, not business logic)
  'try {',
  'try:',
  'catch(',
  'except',
  'finally',
  'error::',
  '.catch(',
  '.then(',

  // Decorators & annotations (infrastructure)
  '@app.',
  '@celery',
  '@task',
  '@route',
  '@middleware',
  '@inject',
  '#[tokio',
  '#[async',
  '#[derive',

  // Exports of infrastructure
  'export default',
  'module.exports',
  'pub mod',
  'pub use',
];

// Keywords/patterns that indicate business logic
const businessLogicKeywords = [
  // Function definitions with business meaning
  'async fn',
  'fn ',
  'def ',
  'function ',
  '=>',

  // Business operations
  'create',
  'update',
  'delete',
  'save',
  'find',
  'fetch',
  'load',
  'send',
  'notify',
  'process',
  'handle',
  'validate',
  'transform',
  'calculate',
  'compute',
  'generate',
  'parse',
  'format',
  'convert',
  'filter',
  'map',
  'reduce',
  'sort',
  'group',
  'aggregate',

  // API/service calls (the actual work)
  'await ',
  '.await',
  'trigger',
  'execute',
  'call',
  'request',
  'getuser',
  'createuser',
  'updateuser',
  'deleteuser',
  'sendemail',
  'sendnotification',
  'publishevent',

  // Control flow for business rules
  'if ',
  'else ',
  'match ',
  'switch',
  'case ',
  'for ',
  'while ',
  'loop ',
  '.foreach',
  '.map(',

  // Return values (actual results)
  'return ',
  'ok(',
  'err(',
  'some(',
  'none',

  // Data manipulation
  'json',
  'serialize',
  'deserialize',
  'encode',
  'decode',
  'push',
  'pop',
  'insert',
  'append',
  'extend',

  // iii/Motia specific patterns
  'emit(',
  'getcontext',
  'registerfunction',
  'invokefunctionasync',
  'bridge.',
  'step.',
  'flow.',
];

interface HighlightedCodeBlockProps {
  code: string;
  title: string;
  tools?: string[];
  variant: 'traditional' | 'iii';
  isDarkMode: boolean;
  language?: string;
  animationPhase: AnimationPhase;
  titleRowHeightPx?: number;
  onTitleRowHeightChange?: (height: number) => void;
}

interface IIIHeaderGraphProps {
  isDarkMode: boolean;
  dependencies: string[];
}

interface HeaderOrbParticle {
  id: number;
  direction: 'right' | 'left';
  duration: number;
  delay: number;
  sizePx: number;
  offsetY: number;
  opacity: number;
}

const IIIHeaderGraph: React.FC<IIIHeaderGraphProps> = ({
  isDarkMode,
  dependencies,
}) => {
  const boxClasses = isDarkMode
    ? 'border-iii-light/60 bg-iii-dark text-iii-light'
    : 'border-iii-dark/40 bg-white text-iii-black';
  const lineClasses = isDarkMode ? 'bg-iii-light/45' : 'bg-iii-dark/35';
  const dotClasses = isDarkMode ? 'bg-iii-info' : 'bg-iii-accent-light';
  const nodes = dependencies.slice(0, 2);
  const nodeSignature = nodes.join('|');
  const [orbs, setOrbs] = useState<HeaderOrbParticle[]>([]);
  const [arePillsVisible, setArePillsVisible] = useState(false);
  const [isOnlineFlashVisible, setIsOnlineFlashVisible] = useState(false);
  const orbIdRef = useRef(0);

  useEffect(() => {
    if (nodes.length === 0) {
      setArePillsVisible(false);
      setIsOnlineFlashVisible(false);
      return;
    }

    setArePillsVisible(false);
    setIsOnlineFlashVisible(false);

    const riseTimer = setTimeout(() => {
      setArePillsVisible(true);
      setIsOnlineFlashVisible(true);
    }, 24);
    const flashTimer = setTimeout(() => {
      setIsOnlineFlashVisible(false);
    }, 760);

    return () => {
      clearTimeout(riseTimer);
      clearTimeout(flashTimer);
    };
  }, [nodeSignature]);

  useEffect(() => {
    if (nodes.length === 0) {
      setOrbs([]);
      return;
    }

    let cancelled = false;
    let timeoutId: ReturnType<typeof setTimeout> | null = null;

    const spawnOrbs = () => {
      const spawnCount = 1 + Math.floor(Math.random() * 3);
      setOrbs((prev) => {
        const next = [...prev];
        for (let i = 0; i < spawnCount; i++) {
          next.push({
            id: orbIdRef.current++,
            direction: Math.random() > 0.5 ? 'right' : 'left',
            duration: 1.1 + Math.random() * 1.2,
            delay: Math.random() * 0.08,
            sizePx: 4 + Math.floor(Math.random() * 3),
            offsetY: -2 + Math.random() * 4,
            opacity: 0.65 + Math.random() * 0.35,
          });
        }
        return next.slice(-64);
      });
    };

    const schedule = () => {
      const nextInMs = 90 + Math.random() * 170;
      timeoutId = setTimeout(() => {
        if (cancelled) {
          return;
        }
        spawnOrbs();
        schedule();
      }, nextInMs);
    };

    spawnOrbs();
    schedule();

    return () => {
      cancelled = true;
      if (timeoutId) {
        clearTimeout(timeoutId);
      }
      setOrbs([]);
    };
  }, [nodes.length, nodeSignature]);

  if (nodes.length === 0) {
    return null;
  }

  return (
    <div className="relative flex items-center gap-2 min-w-0 flex-1">
      <div className={`absolute left-0 right-0 top-1/2 h-px ${lineClasses}`}>
        {orbs.map((orb) => (
          <div
            key={orb.id}
            className={`absolute top-1/2 -translate-y-1/2 rounded-full ${dotClasses}`}
            style={{
              width: `${orb.sizePx}px`,
              height: `${orb.sizePx}px`,
              marginTop: `${orb.offsetY}px`,
              opacity: orb.opacity,
              animation: `${
                orb.direction === 'right' ? 'orbTravelRight' : 'orbTravelLeft'
              } ${orb.duration}s cubic-bezier(0.42, 0, 0.58, 1) ${orb.delay}s 1`,
            }}
            onAnimationEnd={() =>
              setOrbs((prev) => prev.filter((item) => item.id !== orb.id))
            }
          />
        ))}
      </div>

      <div className="relative z-10 ml-auto flex items-center gap-2 min-w-0 flex-nowrap">
        {nodes.map((node, index) => (
          <div
            key={`${node}-${index}`}
            className={`px-2 sm:px-3 py-1 rounded-2xl border text-[11px] sm:text-xs font-semibold overflow-hidden ${boxClasses} ${
              isOnlineFlashVisible
                ? isDarkMode
                  ? 'border-iii-success/80 bg-iii-success/20'
                  : 'border-iii-success/70 bg-iii-success/10'
                : ''
            }`}
            style={{
              maxWidth: 'clamp(104px, 18vw, 220px)',
              transform: `${arePillsVisible ? 'translateY(0px)' : 'translateY(10px)'} ${isOnlineFlashVisible ? 'scale(1.02)' : 'scale(1)'}`,
              opacity: arePillsVisible ? 1 : 0,
              boxShadow: isOnlineFlashVisible
                ? '0 0 0 1px rgba(34, 197, 94, 0.65), 0 0 14px rgba(34, 197, 94, 0.45)'
                : undefined,
              transition:
                'transform 320ms ease-in, opacity 320ms ease-in, background-color 220ms ease-in, border-color 220ms ease-in, box-shadow 220ms ease-in',
            }}
            title={node}
          >
            <div className="flex items-center gap-1.5 min-w-0">
              <span className="inline-flex items-center gap-1 text-[9px] sm:text-[10px] font-semibold text-iii-success flex-shrink-0">
                <span className="w-1.5 h-1.5 rounded-full bg-iii-success" />
              </span>
              <span className="truncate">{node}</span>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
};

// Helper to check if a line contains any tool-related import or setup
const lineContainsTool = (lineText: string, tools: string[]): boolean => {
  const lowerLine = lineText.toLowerCase();
  // Check for import statements or tool names
  if (
    lowerLine.includes('import ') ||
    lowerLine.includes('require(') ||
    lowerLine.includes('from ')
  ) {
    return true;
  }
  // Check for common setup patterns
  if (
    lowerLine.includes('new ') &&
    (lowerLine.includes('redis') ||
      lowerLine.includes('bull') ||
      lowerLine.includes('queue') ||
      lowerLine.includes('client'))
  ) {
    return true;
  }
  // Check for tool-specific keywords
  const toolKeywords = [
    'redis',
    'queue',
    'kafka',
    'rabbit',
    'socket',
    'io',
    'pusher',
    'temporal',
    'cron',
    'agenda',
    'winston',
    'pino',
    'logger',
    'tracer',
    'langchain',
    'openai',
    'celery',
    'airflow',
    'dag',
    'convex',
    'launchdarkly',
    'colyseus',
    'photon',
    'createclient',
    'createadapter',
    'ioredis',
  ];
  return toolKeywords.some((keyword) => lowerLine.includes(keyword));
};

const iiiBuiltInPrefixes = ['state::', 'stream::', 'engine::'];

const extractWorkerServiceDependencies = (code: string): string[] => {
  const localFunctionIds = new Set<string>();
  const functionIdPattern = /function_id\s*:\s*["']([^"']+)["']/g;
  const localIdPattern = /registerFunction\(\s*\{\s*id:\s*["']([^"']+)["']/g;

  for (const match of code.matchAll(localIdPattern)) {
    localFunctionIds.add(match[1]);
  }

  const dependencies = new Set<string>();
  for (const match of code.matchAll(functionIdPattern)) {
    const functionId = match[1];
    if (
      !functionId.includes('::') ||
      functionId === 'publish' ||
      localFunctionIds.has(functionId) ||
      iiiBuiltInPrefixes.some((prefix) => functionId.startsWith(prefix))
    ) {
      continue;
    }
    const [service] = functionId.split('::');
    dependencies.add(service);
  }

  return Array.from(dependencies);
};

const withIIIBaseDependency = (dependencies: string[]): string[] => {
  const iiiModuleDependency = 'iii modules';
  return [
    iiiModuleDependency,
    ...dependencies.filter((dependency) => dependency !== iiiModuleDependency),
  ];
};

const HighlightedCodeBlock: React.FC<HighlightedCodeBlockProps> = ({
  code,
  title,
  tools = [],
  variant,
  isDarkMode,
  language = 'typescript',
  animationPhase,
  titleRowHeightPx,
  onTitleRowHeightChange,
}) => {
  const isTraditional = variant === 'traditional';
  const isIII = variant === 'iii';
  const headerTitle = isIII ? 'iii' : title;

  // Track revealed lines for animation
  const [revealedLine, setRevealedLine] = useState(0);
  const [scanComplete, setScanComplete] = useState(false);
  const totalLinesRef = useRef(0);
  const titleRowRef = useRef<HTMLDivElement>(null);

  // Animation phases
  const isOutputting = ['outputting', 'legendVisible', 'spotlight'].includes(
    animationPhase,
  );
  const isSpotlight = animationPhase === 'spotlight';

  const lines = code.trim().split('\n');
  totalLinesRef.current = lines.length;

  // Animate lines revealing from bottom to top when spotlight starts
  useEffect(() => {
    if (isSpotlight && revealedLine === 0) {
      // Start revealing from bottom
      const totalLines = totalLinesRef.current;
      let currentLine = totalLines;
      setScanComplete(false);

      const interval = setInterval(() => {
        currentLine--;
        setRevealedLine(totalLines - currentLine);

        if (currentLine <= 0) {
          clearInterval(interval);
          setScanComplete(true);
        }
      }, 60); // 60ms per line for smooth animation

      return () => clearInterval(interval);
    } else if (!isSpotlight) {
      setRevealedLine(0);
      setScanComplete(false);
    }
  }, [isSpotlight]);

  // Detect architecture lines (alert) and business logic lines (accent)
  const architectureLines = new Set<number>();
  const businessLogicLines = new Set<number>();

  lines.forEach((line, i) => {
    const lowerLine = line.toLowerCase().trim();
    const lineNum = i + 1;

    // Skip empty lines, comments, and pure braces
    if (
      lowerLine.length < 2 ||
      lowerLine === '}' ||
      lowerLine === '};' ||
      lowerLine === '{' ||
      lowerLine.startsWith('//') ||
      lowerLine.startsWith('/*') ||
      lowerLine.startsWith('*') ||
      (lowerLine.startsWith('#') && !lowerLine.startsWith('#[')) // Python comments, but not Rust attributes
    ) {
      return;
    }

    // Check if line contains architecture keywords
    const isArchitecture = architectureKeywords.some((kw) =>
      lowerLine.includes(kw.toLowerCase()),
    );

    // Check if line contains business logic keywords
    const hasBusinessLogic = businessLogicKeywords.some((kw) =>
      lowerLine.includes(kw.toLowerCase()),
    );

    // Strong architecture indicators (these always win)
    const isStrongArchitecture =
      lowerLine.startsWith('import ') ||
      lowerLine.startsWith('from ') ||
      lowerLine.startsWith('use ') ||
      lowerLine.startsWith('extern ') ||
      lowerLine.includes('require(') ||
      lowerLine.includes('process.env') ||
      lowerLine.includes('os.environ') ||
      lowerLine.includes('std::env') ||
      lowerLine.includes('.env') ||
      lowerLine.startsWith('try ') ||
      lowerLine.startsWith('try:') ||
      lowerLine.startsWith('try{') ||
      lowerLine.includes('catch(') ||
      lowerLine.includes('catch (') ||
      lowerLine.startsWith('except') ||
      lowerLine.startsWith('finally') ||
      lowerLine.includes('.on(') ||
      lowerLine.includes('.once(') ||
      lowerLine.includes('.subscribe(') ||
      lowerLine.includes('addeventlistener') ||
      lowerLine.includes('.then(') ||
      lowerLine.includes('.catch(') ||
      lowerLine.includes('new redis') ||
      lowerLine.includes('new bull') ||
      lowerLine.includes('createclient') ||
      lowerLine.includes('connect(') ||
      lowerLine.includes('init(') ||
      lowerLine.startsWith('@') || // Decorators
      lowerLine.startsWith('#['); // Rust attributes

    // Strong business logic indicators
    const isStrongBusinessLogic =
      lowerLine.includes('emit(') ||
      lowerLine.includes('getcontext') ||
      lowerLine.includes('registerfunction') ||
      lowerLine.includes('registertrigger') ||
      lowerLine.includes('triggervoid') ||
      lowerLine.includes('iii.trigger(') ||
      lowerLine.includes('invokefunctionasync') ||
      lowerLine.includes('state::') ||
      lowerLine.includes('stream::') ||
      lowerLine.includes("'publish'") ||
      lowerLine.includes('"publish"') ||
      lowerLine.includes("'enqueue'") ||
      lowerLine.includes('"enqueue"') ||
      (lowerLine.includes('await ') &&
        !lowerLine.includes('connect') &&
        !lowerLine.includes('subscribe')) ||
      (lowerLine.includes('return ') && !lowerLine.includes('error')) ||
      lowerLine.includes('createuser') ||
      lowerLine.includes('sendemail') ||
      lowerLine.includes('sendnotification');

    // Categorize the line
    if (isStrongArchitecture) {
      architectureLines.add(lineNum);
    } else if (isStrongBusinessLogic) {
      businessLogicLines.add(lineNum);
    } else if (isArchitecture && !hasBusinessLogic) {
      architectureLines.add(lineNum);
    } else if (hasBusinessLogic && !isArchitecture) {
      businessLogicLines.add(lineNum);
    } else if (hasBusinessLogic && isArchitecture) {
      // Both present - use heuristics
      // If it's defining a handler/listener, it's architecture
      if (
        lowerLine.includes('handler') ||
        lowerLine.includes('listener') ||
        lowerLine.includes('middleware')
      ) {
        architectureLines.add(lineNum);
      } else {
        businessLogicLines.add(lineNum);
      }
    }
  });

  // For traditional: during early phases, highlight architecture as "bad" (alert)
  const shouldHighlight =
    isTraditional &&
    (animationPhase === 'highlighting' || animationPhase === 'moving');
  const shouldShowExtracted = isTraditional && animationPhase === 'moving';

  const inferredDependencies = Array.from(
    new Set(
      tools
        .filter((tool) => {
          const tokens = tool
            .toLowerCase()
            .split(/[^a-z0-9]+/)
            .filter(
              (token) => token.length >= 3 && token !== 'sdk' && token !== 'js',
            );
          if (tokens.length === 0) {
            return false;
          }
          return lines.some((line, index) => {
            if (!architectureLines.has(index + 1)) {
              return false;
            }
            const lowerLine = line.toLowerCase();
            return tokens.some((token) => lowerLine.includes(token));
          });
        })
        .map((tool) => tool.replace(/\s*\+.*$/, '').trim())
        .filter(Boolean),
    ),
  );
  const baseDependencies = isIII
    ? withIIIBaseDependency(extractWorkerServiceDependencies(code))
    : inferredDependencies.length > 0
      ? inferredDependencies
      : tools
          .map((tool) => tool.replace(/\s*\+.*$/, '').trim())
          .filter(Boolean);
  const dependencyCount = [
    'moving',
    'linking',
    'connected',
    'outputting',
    'legendVisible',
    'spotlight',
  ].includes(animationPhase)
    ? 2
    : 0;
  const headerDependencies = baseDependencies.slice(0, dependencyCount);

  useEffect(() => {
    if (!onTitleRowHeightChange || !titleRowRef.current) {
      return;
    }

    const titleRowElement = titleRowRef.current;
    const reportHeight = () => {
      const height = Math.ceil(titleRowElement.getBoundingClientRect().height);
      if (height > 0) {
        onTitleRowHeightChange(height);
      }
    };

    reportHeight();

    const resizeObserver = new ResizeObserver(reportHeight);
    resizeObserver.observe(titleRowElement);

    return () => resizeObserver.disconnect();
  }, [animationPhase, onTitleRowHeightChange]);

  // Border styling for iii code block - transforms to dashed accent when outputting
  const getBorderClasses = () => {
    if (isIII && isOutputting) {
      return isDarkMode
        ? 'border-iii-accent border-dashed'
        : 'border-iii-accent-light border-dashed';
    }
    return isDarkMode ? 'border-iii-light' : 'border-iii-dark';
  };
  return (
    <div
      className={`rounded-lg overflow-hidden border-2 h-full flex flex-col transition-all duration-700 ${getBorderClasses()} ${
        isDarkMode ? 'bg-iii-black' : 'bg-white'
      } ${
        isIII && isOutputting
          ? isDarkMode
            ? 'shadow-lg shadow-iii-accent/20'
            : 'shadow-lg shadow-iii-accent-light/20'
          : ''
      }`}
    >
      {/* Header */}
      <div
        className={`flex flex-col gap-2 px-3 sm:px-4 py-2 sm:py-3 border-b transition-colors duration-300 flex-shrink-0 ${
          isDarkMode
            ? 'border-iii-light bg-iii-dark/50'
            : 'border-iii-dark bg-iii-light/50'
        }`}
      >
        <div
          ref={titleRowRef}
          className="flex items-center gap-2"
          style={
            titleRowHeightPx ? { height: `${titleRowHeightPx}px` } : undefined
          }
        >
          <div className="flex items-center gap-2 min-w-0">
            <div
              className={`w-2.5 h-2.5 rounded-full flex-shrink-0 ${
                isTraditional
                  ? 'bg-iii-light'
                  : isDarkMode
                    ? 'bg-iii-accent'
                    : 'bg-iii-accent-light'
              }`}
            />
            <span
              className={`text-xs sm:text-sm font-medium transition-colors duration-300 truncate ${
                isDarkMode ? 'text-iii-light' : 'text-iii-black'
              }`}
            >
              {headerTitle}
            </span>
          </div>
          {isIII && (
            <IIIHeaderGraph
              isDarkMode={isDarkMode}
              dependencies={headerDependencies}
            />
          )}
        </div>
      </div>

      {/* Code */}
      <div
        className={`p-2 sm:p-3 md:p-4 overflow-auto flex-1 max-h-[400px] sm:max-h-[500px] relative ${
          isDarkMode ? 'scrollbar-brand-dark' : 'scrollbar-brand-light'
        }`}
      >
        {/* Scan line effect during spotlight - constrained to marker area */}
        {isSpotlight &&
          revealedLine > 0 &&
          revealedLine < totalLinesRef.current && (
            <div
              className="absolute left-0 w-1 h-1 pointer-events-none z-10"
              style={{
                bottom: `${(revealedLine / totalLinesRef.current) * 100}%`,
                width: '4px',
                height: '3px',
                borderRadius: '2px',
                background: isDarkMode
                  ? 'var(--color-accent)'
                  : 'var(--color-accent-light)',
              }}
            />
          )}

        <Highlight
          theme={isDarkMode ? themes.nightOwl : themes.github}
          code={code.trim()}
          language={language as any}
        >
          {({ tokens, getLineProps, getTokenProps }) => {
            const totalLines = tokens.length;

            return (
              <pre className="text-[9px] sm:text-[10px] md:text-xs font-mono leading-relaxed overflow-x-auto">
                {tokens.map((line, i) => {
                  const lineNum = i + 1;
                  const isArchLine = architectureLines.has(lineNum);
                  const isBizLine = businessLogicLines.has(lineNum);

                  // Traditional: during early phases show architecture as alert (red)
                  const showTraditionalHighlight =
                    shouldHighlight && isArchLine;
                  const extracted = shouldShowExtracted && isArchLine;

                  // Calculate if this line has been revealed (bottom to top)
                  const lineFromBottom = totalLines - lineNum + 1;
                  const isRevealed = revealedLine >= lineFromBottom;

                  // Spotlight phase: show markers only when revealed
                  const showArchHighlight =
                    isSpotlight && isArchLine && isRevealed;
                  const showBizHighlight =
                    isSpotlight && isBizLine && isRevealed;

                  return (
                    <div
                      key={i}
                      {...getLineProps({ line })}
                      className={`
                      whitespace-pre relative transition-all duration-300
                      ${extracted ? 'translate-x-2 scale-[0.98]' : ''}
                      ${
                        showTraditionalHighlight
                          ? 'rounded-sm bg-iii-alert/10'
                          : ''
                      }
                      ${showArchHighlight ? 'rounded-sm bg-iii-warn/10' : ''}
                      ${showBizHighlight ? 'rounded-sm bg-iii-success/10' : ''}
                    `}
                    >
                      {/* Initial push: alert (red) */}
                      {showTraditionalHighlight && (
                        <span className="absolute left-0 top-0 bottom-0 w-0.5 rounded-full bg-iii-alert" />
                      )}

                      {/* Spotlight architecture: warn (orange) */}
                      {showArchHighlight && (
                        <span
                          className={`absolute left-0 top-0 bottom-0 w-0.5 rounded-full bg-iii-warn ${
                            !scanComplete ? 'animate-pulse' : ''
                          }`}
                        />
                      )}

                      {/* Business logic indicator bar (success/green) */}
                      {showBizHighlight && (
                        <span
                          className={`absolute left-0 top-0 bottom-0 w-0.5 rounded-full bg-iii-success ${
                            !scanComplete ? 'animate-pulse' : ''
                          }`}
                        />
                      )}

                      <span
                        className={`inline-block w-6 sm:w-8 text-right mr-2 sm:mr-3 select-none ${
                          isDarkMode
                            ? 'text-iii-light/30'
                            : 'text-iii-medium/40'
                        }`}
                      >
                        {lineNum}
                      </span>
                      {line.map((token, key) => (
                        <span key={key} {...getTokenProps({ token })} />
                      ))}
                    </div>
                  );
                })}
              </pre>
            );
          }}
        </Highlight>
      </div>
    </div>
  );
};

interface DependencyVisualizationProps {
  traditionalCode: string;
  traditionalTitle: string;
  traditionalTools: string[];
  traditionalLanguage: string;
  iiiCode: string;
  iiiTitle: string;
  iiiLanguage: string;
  categoryId: string;
  isDarkMode: boolean;
}

export const DependencyVisualization: React.FC<
  DependencyVisualizationProps
> = ({
  traditionalCode,
  traditionalTitle,
  traditionalTools,
  traditionalLanguage,
  iiiCode,
  iiiTitle,
  iiiLanguage,
  categoryId,
  isDarkMode,
}) => {
  const [animationPhase, setAnimationPhase] = useState<AnimationPhase>('idle');
  const [hasAnimatedOnScroll, setHasAnimatedOnScroll] = useState(false);
  const [sharedTitleRowHeightPx, setSharedTitleRowHeightPx] = useState(30);
  const containerRef = useRef<HTMLDivElement>(null);

  const handleTitleRowHeightChange = useCallback((height: number) => {
    if (height <= 0) {
      return;
    }
    setSharedTitleRowHeightPx((previousHeight) =>
      previousHeight === height ? previousHeight : height,
    );
  }, []);

  const runAnimation = useCallback(() => {
    setAnimationPhase('idle');

    // Phase 1: Highlight dependency code in traditional block
    const timer1 = setTimeout(() => setAnimationPhase('highlighting'), 300);

    // Phase 2: Transition dependency emphasis
    const timer2 = setTimeout(() => setAnimationPhase('moving'), 1500);

    // Phase 3: Activate linked visualization state
    const timer3 = setTimeout(() => setAnimationPhase('linking'), 3200);

    // Phase 4: Show fully connected state (color change completes)
    const timer4 = setTimeout(() => setAnimationPhase('connected'), 4500);

    // Phase 5: Output to iii code (decoupled connection)
    const timer5 = setTimeout(() => setAnimationPhase('outputting'), 5800);

    // Phase 6: Legend appears first
    const timer6 = setTimeout(() => setAnimationPhase('legendVisible'), 7000);

    // Phase 7: Spotlight - code markers animate in from bottom to top
    const timer7 = setTimeout(() => setAnimationPhase('spotlight'), 8500);

    return () => {
      clearTimeout(timer1);
      clearTimeout(timer2);
      clearTimeout(timer3);
      clearTimeout(timer4);
      clearTimeout(timer5);
      clearTimeout(timer6);
      clearTimeout(timer7);
    };
  }, []);

  // Run animation on scroll into view (once)
  useEffect(() => {
    const observer = new IntersectionObserver(
      (entries) => {
        entries.forEach((entry) => {
          if (entry.isIntersecting && !hasAnimatedOnScroll) {
            setHasAnimatedOnScroll(true);
          }
        });
      },
      { threshold: 0.2 },
    );

    if (containerRef.current) {
      observer.observe(containerRef.current);
    }

    return () => observer.disconnect();
  }, [hasAnimatedOnScroll]);

  // Re-run animation when category changes
  useEffect(() => {
    if (!hasAnimatedOnScroll) {
      return;
    }

    const cleanup = runAnimation();
    return cleanup;
  }, [categoryId, hasAnimatedOnScroll, runAnimation]);

  const isLegendVisible =
    animationPhase === 'legendVisible' || animationPhase === 'spotlight';
  const isSpotlight = animationPhase === 'spotlight';

  return (
    <div ref={containerRef} className="space-y-4">
      {/* Desktop layout: Traditional | iii */}
      <div className="hidden lg:grid grid-cols-2 gap-4 xl:gap-6 overflow-visible">
        {/* Traditional Code - Left */}
        <div className="min-w-0">
          <HighlightedCodeBlock
            code={traditionalCode}
            title={traditionalTitle}
            tools={traditionalTools}
            variant="traditional"
            isDarkMode={isDarkMode}
            language={traditionalLanguage}
            animationPhase={animationPhase}
            titleRowHeightPx={sharedTitleRowHeightPx}
          />
        </div>

        {/* iii Code - Right */}
        <div className="min-w-0">
          <HighlightedCodeBlock
            code={iiiCode}
            title={iiiTitle}
            tools={traditionalTools}
            variant="iii"
            isDarkMode={isDarkMode}
            language={iiiLanguage}
            animationPhase={animationPhase}
            titleRowHeightPx={sharedTitleRowHeightPx}
            onTitleRowHeightChange={handleTitleRowHeightChange}
          />
        </div>
      </div>

      {/* Mobile/Tablet: Simple stacked view (no animation complexity) */}
      <div className="lg:hidden flex flex-col gap-4">
        <HighlightedCodeBlock
          code={traditionalCode}
          title={traditionalTitle}
          tools={traditionalTools}
          variant="traditional"
          isDarkMode={isDarkMode}
          language={traditionalLanguage}
          animationPhase="idle"
        />
        <HighlightedCodeBlock
          code={iiiCode}
          title={iiiTitle}
          tools={traditionalTools}
          variant="iii"
          isDarkMode={isDarkMode}
          language={iiiLanguage}
          animationPhase="idle"
        />
      </div>

      {/* Legend - aligned below respective code blocks */}
      <div
        className={`
          hidden lg:grid grid-cols-2 gap-4 xl:gap-6
          transition-all duration-1000 ease-out overflow-hidden
          ${isLegendVisible ? 'max-h-32 opacity-100' : 'max-h-0 opacity-0'}
        `}
      >
        {/* Architecture legend - below traditional code (left) */}
        <div className="flex justify-center">
          <div
            className={`
              flex items-center gap-3 px-5 py-3 rounded-lg
              transition-all duration-700 ease-out
              ${
                isLegendVisible
                  ? 'opacity-100 translate-y-0'
                  : 'opacity-0 translate-y-4'
              }
              ${
                isDarkMode
                  ? 'bg-iii-dark/80 shadow-lg shadow-black/20'
                  : 'bg-white/80 shadow-lg shadow-black/5'
              }
            `}
            style={{
              transitionDelay: isLegendVisible ? '400ms' : '0ms',
              backdropFilter: 'blur(8px)',
            }}
          >
            <div className="w-4 h-4 rounded bg-iii-warn" />
            <div className="flex flex-col">
              <span className="text-sm font-medium text-iii-warn">
                Architecture
              </span>
              <span
                className={`text-[10px] ${
                  isDarkMode ? 'text-iii-light/50' : 'text-iii-medium/70'
                }`}
              >
                Infrastructure & setup code
              </span>
            </div>
          </div>
        </div>

        {/* Business Logic legend - below iii code (right) */}
        <div className="flex justify-center">
          <div
            className={`
              flex items-center gap-3 px-5 py-3 rounded-lg
              transition-all duration-700 ease-out
              ${
                isLegendVisible
                  ? 'opacity-100 translate-y-0'
                  : 'opacity-0 translate-y-4'
              }
              ${
                isDarkMode
                  ? 'bg-iii-dark/80 shadow-lg shadow-black/20'
                  : 'bg-white/80 shadow-lg shadow-black/5'
              }
            `}
            style={{
              transitionDelay: isLegendVisible ? '600ms' : '0ms',
              backdropFilter: 'blur(8px)',
            }}
          >
            <div className="w-4 h-4 rounded bg-iii-success" />
            <div className="flex flex-col">
              <span className="text-sm font-medium text-iii-success">
                Business Logic
              </span>
              <span
                className={`text-[10px] ${
                  isDarkMode ? 'text-iii-light/50' : 'text-iii-medium/70'
                }`}
              >
                Your application code
              </span>
            </div>
          </div>
        </div>
      </div>

      {/* Mobile legend - stacked centered */}
      <div
        className={`
          lg:hidden flex justify-center transition-all duration-1000 ease-out overflow-hidden
          ${isLegendVisible ? 'max-h-32 opacity-100' : 'max-h-0 opacity-0'}
        `}
      >
        <div
          className={`
            flex flex-col sm:flex-row items-center gap-4 sm:gap-6 px-5 py-3 rounded-lg
            transition-all duration-700 ease-out
            ${isLegendVisible ? 'opacity-100 translate-y-0' : 'opacity-0 translate-y-4'}
            ${isDarkMode ? 'bg-iii-dark/80 shadow-lg shadow-black/20' : 'bg-white/80 shadow-lg shadow-black/5'}
          `}
          style={{ backdropFilter: 'blur(8px)' }}
        >
          <div className="flex items-center gap-3">
            <div className="w-4 h-4 rounded bg-iii-warn" />
            <span className="text-sm font-medium text-iii-warn">
              Architecture
            </span>
          </div>
          <div className="flex items-center gap-3">
            <div className="w-4 h-4 rounded bg-iii-success" />
            <span className="text-sm font-medium text-iii-success">
              Business Logic
            </span>
          </div>
        </div>
      </div>
    </div>
  );
};
