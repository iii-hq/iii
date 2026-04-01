import { Highlight, themes } from 'prism-react-renderer';

interface HelloWorldSectionProps {
  isDarkMode?: boolean;
}

// Language Icons
const PythonIcon = () => (
  <svg viewBox="0 0 24 24" className="w-5 h-5" fill="currentColor">
    <path d="M14.25.18l.9.2.73.26.59.3.45.32.34.34.25.34.16.33.1.3.04.26.02.2-.01.13V8.5l-.05.63-.13.55-.21.46-.26.38-.3.31-.33.25-.35.19-.35.14-.33.1-.3.07-.26.04-.21.02H8.77l-.69.05-.59.14-.5.22-.41.27-.33.32-.27.35-.2.36-.15.37-.1.35-.07.32-.04.27-.02.21v3.06H3.17l-.21-.03-.28-.07-.32-.12-.35-.18-.36-.26-.36-.36-.35-.46-.32-.59-.28-.73-.21-.88-.14-1.05-.05-1.23.06-1.22.16-1.04.24-.87.32-.71.36-.57.4-.44.42-.33.42-.24.4-.16.36-.1.32-.05.24-.01h.16l.06.01h8.16v-.83H6.18l-.01-2.75-.02-.37.05-.34.11-.31.17-.28.25-.26.31-.23.38-.2.44-.18.51-.15.58-.12.64-.1.71-.06.77-.04.84-.02 1.27.05zm-6.3 1.98l-.23.33-.08.41.08.41.23.34.33.22.41.09.41-.09.33-.22.23-.34.08-.41-.08-.41-.23-.33-.33-.22-.41-.09-.41.09zm13.09 3.95l.28.06.32.12.35.18.36.27.36.35.35.47.32.59.28.73.21.88.14 1.04.05 1.23-.06 1.23-.16 1.04-.24.86-.32.71-.36.57-.4.45-.42.33-.42.24-.4.16-.36.09-.32.05-.24.02-.16-.01h-8.22v.82h5.84l.01 2.76.02.36-.05.34-.11.31-.17.29-.25.25-.31.24-.38.2-.44.17-.51.15-.58.13-.64.09-.71.07-.77.04-.84.01-1.27-.04-1.07-.14-.9-.2-.73-.25-.59-.3-.45-.33-.34-.34-.25-.34-.16-.33-.1-.3-.04-.25-.02-.2.01-.13v-5.34l.05-.64.13-.54.21-.46.26-.38.3-.32.33-.24.35-.2.35-.14.33-.1.3-.06.26-.04.21-.02.13-.01h5.84l.69-.05.59-.14.5-.21.41-.28.33-.32.27-.35.2-.36.15-.36.1-.35.07-.32.04-.28.02-.21V6.07h2.09l.14.01zm-6.47 14.25l-.23.33-.08.41.08.41.23.33.33.23.41.08.41-.08.33-.23.23-.33.08-.41-.08-.41-.08-.41.08z" />
  </svg>
);

// Rust Logo - Official icon from Simple Icons
const RustIcon = () => (
  <svg viewBox="0 0 24 24" className="w-5 h-5" fill="currentColor">
    <path d="M23.8346 11.7033l-1.0073-.6236a13.7268 13.7268 0 00-.0283-.2936l.8656-.8069a.3483.3483 0 00-.1154-.578l-1.1066-.414a8.4958 8.4958 0 00-.087-.2856l.6904-.9587a.3462.3462 0 00-.2257-.5446l-1.1663-.1894a9.3574 9.3574 0 00-.1407-.2622l.49-1.0761a.3437.3437 0 00-.0274-.3361.3486.3486 0 00-.3006-.154l-1.1845.0416a6.7444 6.7444 0 00-.1873-.2268l.2723-1.153a.3472.3472 0 00-.417-.4172l-1.1532.2724a14.0183 14.0183 0 00-.2278-.1873l.0415-1.1845a.3442.3442 0 00-.49-.328l-1.076.491c-.0872-.0476-.1742-.0952-.2623-.1407l-.1903-1.1673A.3483.3483 0 0016.256.955l-.9597.6905a8.4867 8.4867 0 00-.2855-.086l-.414-1.1066a.3483.3483 0 00-.5781-.1154l-.8069.8666a9.2936 9.2936 0 00-.2936-.0284L12.2946.1683a.3462.3462 0 00-.5892 0l-.6236 1.0073a13.7383 13.7383 0 00-.2936.0284L9.9803.3374a.3462.3462 0 00-.578.1154l-.4141 1.1065c-.0962.0274-.1903.0567-.2855.086L7.744.955a.3483.3483 0 00-.5447.2258L7.009 2.348a9.3574 9.3574 0 00-.2622.1407l-1.0762-.491a.3462.3462 0 00-.49.328l.0416 1.1845a7.9826 7.9826 0 00-.2278.1873L3.8413 3.425a.3472.3472 0 00-.4171.4171l.2713 1.1531c-.0628.075-.1255.1509-.1863.2268l-1.1845-.0415a.3462.3462 0 00-.328.49l.491 1.0761a9.167 9.167 0 00-.1407.2622l-1.1662.1894a.3483.3483 0 00-.2258.5446l.6904.9587a13.303 13.303 0 00-.087.2855l-1.1065.414a.3483.3483 0 00-.1155.5781l.8656.807a9.2936 9.2936 0 00-.0283.2935l-1.0073.6236a.3442.3442 0 000 .5892l1.0073.6236c.008.0982.0182.1964.0283.2936l-.8656.8079a.3462.3462 0 00.1155.578l1.1065.4141c.0273.0962.0567.1914.087.2855l-.6904.9587a.3452.3452 0 00.2268.5447l1.1662.1893c.0456.088.0922.1751.1408.2622l-.491 1.0762a.3462.3462 0 00.328.49l1.1834-.0415c.0618.0769.1235.1528.1873.2277l-.2713 1.1541a.3462.3462 0 00.4171.4161l1.153-.2713c.075.0638.151.1255.2279.1863l-.0415 1.1845a.3442.3442 0 00.49.327l1.0761-.49c.087.0486.1741.0951.2622.1407l.1903 1.1662a.3483.3483 0 00.5447.2268l.9587-.6904a9.299 9.299 0 00.2855.087l.414 1.1066a.3452.3452 0 00.5781.1154l.8079-.8656c.0972.0111.1954.0203.2936.0294l.6236 1.0073a.3472.3472 0 00.5892 0l.6236-1.0073c.0982-.0091.1964-.0183.2936-.0294l.8069.8656a.3483.3483 0 00.578-.1154l.4141-1.1066a8.4626 8.4626 0 00.2855-.087l.9587.6904a.3452.3452 0 00.5447-.2268l.1903-1.1662c.088-.0456.1751-.0931.2622-.1407l1.0762.49a.3472.3472 0 00.49-.327l-.0415-1.1845a6.7267 6.7267 0 00.2267-.1863l1.1531.2713a.3472.3472 0 00.4171-.416l-.2713-1.1542c.0628-.0749.1255-.1508.1863-.2278l1.1845.0415a.3442.3442 0 00.328-.49l-.49-1.076c.0475-.0872.0951-.1742.1407-.2623l1.1662-.1893a.3483.3483 0 00.2258-.5447l-.6904-.9587.087-.2855 1.1066-.414a.3462.3462 0 00.1154-.5781l-.8656-.8079c.0101-.0972.0202-.1954.0283-.2936l1.0073-.6236a.3442.3442 0 000-.5892zm-6.7413 8.3551a.7138.7138 0 01.2986-1.396.714.714 0 11-.2997 1.396zm-.3422-2.3142a.649.649 0 00-.7715.5l-.3573 1.6685c-1.1035.501-2.3285.7795-3.6193.7795a8.7368 8.7368 0 01-3.6951-.814l-.3574-1.6684a.648.648 0 00-.7714-.499l-1.473.3158a8.7216 8.7216 0 01-.7613-.898h7.1676c.081 0 .1356-.0141.1356-.088v-2.536c0-.074-.0536-.0881-.1356-.0881h-2.0966v-1.6077h2.2677c.2065 0 1.1065.0587 1.394 1.2088.0901.3533.2875 1.5044.4232 1.8729.1346.413.6833 1.2381 1.2685 1.2381h3.5716a.7492.7492 0 00.1296-.0131 8.7874 8.7874 0 01-.8119.9526zM6.8369 20.024a.714.714 0 11-.2997-1.396.714.714 0 01.2997 1.396zM4.1177 8.9972a.7137.7137 0 11-1.304.5791.7137.7137 0 011.304-.579zm-.8352 1.9813l1.5347-.6824a.65.65 0 00.33-.8585l-.3158-.7147h1.2432v5.6025H3.5669a8.7753 8.7753 0 01-.2834-3.348zm6.7343-.5437V8.7836h2.9601c.153 0 1.0792.1772 1.0792.8697 0 .575-.7107.7815-1.2948.7815zm10.7574 1.4862c0 .2187-.008.4363-.0243.651h-.9c-.09 0-.1265.0586-.1265.1477v.413c0 .973-.5487 1.1846-1.0296 1.2382-.4576.0517-.9648-.1913-1.0275-.4717-.2704-1.5186-.7198-1.8436-1.4305-2.4034.8817-.5599 1.799-1.386 1.799-2.4915 0-1.1936-.819-1.9458-1.3769-2.3153-.7825-.5163-1.6491-.6195-1.883-.6195H5.4682a8.7651 8.7651 0 014.907-2.7699l1.0974 1.151a.648.648 0 00.9182.0213l1.227-1.1743a8.7753 8.7753 0 016.0044 4.2762l-.8403 1.8982a.652.652 0 00.33.8585l1.6178.7188c.0283.2875.0425.577.0425.8717zm-9.3006-9.5993a.7128.7128 0 11.984 1.0316.7137.7137 0 01-.984-1.0316zm8.3389 6.71a.7107.7107 0 01.9395-.3625.7137.7137 0 11-.9405.3635z" />
  </svg>
);

const NodeIcon = () => (
  <svg viewBox="0 0 24 24" className="w-5 h-5" fill="currentColor">
    <path d="M11.998,24c-0.321,0-0.641-0.084-0.922-0.247l-2.936-1.737c-0.438-0.245-0.224-0.332-0.08-0.383 c0.585-0.203,0.703-0.25,1.328-0.604c0.065-0.037,0.151-0.023,0.218,0.017l2.256,1.339c0.082,0.045,0.197,0.045,0.272,0l8.795-5.076 c0.082-0.047,0.134-0.141,0.134-0.238V6.921c0-0.099-0.053-0.192-0.137-0.242l-8.791-5.072c-0.081-0.047-0.189-0.047-0.271,0 L3.075,6.68C2.99,6.729,2.936,6.825,2.936,6.921v10.15c0,0.097,0.054,0.189,0.139,0.235l2.409,1.392 c1.307,0.654,2.108-0.116,2.108-0.89V7.787c0-0.142,0.114-0.253,0.256-0.253h1.115c0.139,0,0.255,0.112,0.255,0.253v10.021 c0,1.745-0.95,2.745-2.604,2.745c-0.508,0-0.909,0-2.026-0.551L2.28,18.675c-0.57-0.329-0.922-0.945-0.922-1.604V6.921 c0-0.659,0.353-1.275,0.922-1.603l8.795-5.082c0.557-0.315,1.296-0.315,1.848,0l8.794,5.082c0.57,0.329,0.924,0.944,0.924,1.603 v10.15c0,0.659-0.354,1.273-0.924,1.604l-8.794,5.078C12.643,23.916,12.324,24,11.998,24z" />
  </svg>
);

// Code snippets
const pythonCode = `import torch
from iii import register_worker

iii = register_worker("ws://localhost:49134")

async def predict(input):
    t = torch.tensor(input["data"])
    result = model(t)
    return {
        "predictions": result.tolist()
    }

iii.register_function(
    "ml::predict", predict
)`;

const rustCode = `use iii_sdk::{
    register_worker, IIIError, TriggerRequest,
    Value,
};
use serde_json::json;

async fn transform(
    input: Value,
) -> Result<Value, IIIError> {
    let nums: Vec<f64> =
        serde_json::from_value(input)?;
    let result: Vec<f64> =
        nums.iter().map(|x| x * 2.0).collect();
    Ok(json!(result))
}

#[tokio::main]
async fn main() -> Result<(), IIIError> {
    let iii = register_worker(
        "ws://localhost:49134",
        InitOptions::default(),
    )?;
    iii.register_function(
        "data::transform",
        transform,
    );
    Ok(())
}`;

const nodeCode = `import { registerWorker } from "iii-sdk"

const iii = registerWorker(
  "ws://localhost:49134"
)

const nums = await iii.trigger({
  function_id: "data::transform",
  payload: [1.0, 2.0, 3.0],
})

const pred = await iii.trigger({
  function_id: "ml::predict",
  payload: { data: nums },
})`;

export function HelloWorldSection({
  isDarkMode = true,
}: HelloWorldSectionProps) {
  const textPrimary = isDarkMode ? 'text-iii-light' : 'text-iii-black';
  const textSecondary = isDarkMode ? 'text-iii-light/70' : 'text-iii-black/70';
  const borderColor = isDarkMode
    ? 'border-iii-light/10'
    : 'border-iii-black/10';
  const bgCard = isDarkMode ? 'bg-iii-dark/30' : 'bg-white/50';
  const accentColor = isDarkMode ? 'text-iii-accent' : 'text-iii-accent-light';
  const accentBg = isDarkMode ? 'bg-iii-accent' : 'bg-iii-accent-light';

  const codeBlocks = [
    {
      title: 'Python Worker',
      subtitle: 'ML Inference',
      icon: PythonIcon,
      code: pythonCode,
      color: 'text-iii-info',
      language: 'python',
    },
    {
      title: 'Rust Worker',
      subtitle: 'Data Transform',
      icon: RustIcon,
      code: rustCode,
      color: 'text-iii-warn',
      language: 'rust',
    },
    {
      title: 'Node.js Worker',
      subtitle: 'Orchestrator',
      icon: NodeIcon,
      code: nodeCode,
      color: 'text-iii-success',
      language: 'typescript',
    },
  ];

  return (
    <section
      className={`relative w-full overflow-hidden font-mono transition-colors duration-300 ${textPrimary}`}
    >
      {/* Subtle ambient glow decoration */}
      <div className="absolute inset-0 pointer-events-none overflow-hidden">
        <div
          className="absolute -top-1/4 -right-1/4 w-1/2 h-1/2 rounded-full opacity-[0.03]"
          style={{
            background:
              'radial-gradient(circle, var(--color-accent) 0%, transparent 70%)',
          }}
        />
        <div
          className="absolute -bottom-1/4 -left-1/4 w-1/3 h-1/3 rounded-full opacity-[0.02]"
          style={{
            background:
              'radial-gradient(circle, var(--color-info) 0%, transparent 70%)',
          }}
        />
      </div>
      <div className="relative z-10 w-full max-w-7xl mx-auto px-4 sm:px-6 py-5 md:py-6">
        {/* Header */}
        <div className="text-center mb-7 md:mb-10 space-y-3">
          <h2 className="text-xl sm:text-3xl md:text-4xl lg:text-5xl font-bold tracking-tighter leading-[1.1]">
            <span className="block sm:inline">One protocol.</span>{' '}
            <span className={`${accentColor} relative inline-block`}>
              Any language.
              <svg
                className="absolute -bottom-1 sm:-bottom-2 left-0 w-full h-1.5 sm:h-2 opacity-30"
                viewBox="0 0 200 8"
                preserveAspectRatio="none"
              >
                <path
                  d="M0 4 Q50 0 100 4 T200 4"
                  stroke="currentColor"
                  strokeWidth="2"
                  fill="none"
                />
              </svg>
            </span>
          </h2>
          <p
            className={`text-sm md:text-base lg:text-lg max-w-3xl mx-auto leading-relaxed ${textSecondary}`}
          >
            Python registers a function. Rust registers a function. Node.js
            consumes both.
            <br className="hidden sm:block" />
            Simply register functions and trigger them.
          </p>
        </div>

        {/* Code Blocks Grid */}
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4 md:gap-6 lg:gap-8">
          {codeBlocks.map((block, index) => (
            <div key={index} className="relative h-full">
              {/* Code Card */}
              <div
                className={`
                relative rounded-lg border-2 ${borderColor} ${bgCard}
                overflow-hidden transition-all duration-300 h-full flex flex-col
                hover:border-opacity-30 hover:shadow-lg
                ${isDarkMode ? 'hover:border-iii-light/20' : 'hover:border-iii-black/20'}
              `}
              >
                {/* Header */}
                <div
                  className={`
                  flex items-center gap-3 px-5 py-4 border-b ${borderColor}
                  ${isDarkMode ? 'bg-iii-dark/50' : 'bg-white/80'}
                `}
                >
                  <div
                    className={`p-2 rounded-lg ${isDarkMode ? 'bg-white/5' : 'bg-black/5'} ${block.color}`}
                  >
                    <block.icon />
                  </div>
                  <div>
                    <div className={`text-sm font-bold ${textPrimary}`}>
                      {block.title}
                    </div>
                    <div className={`text-xs ${textSecondary}`}>
                      {block.subtitle}
                    </div>
                  </div>
                  {/* Step indicator */}
                  <div
                    className={`
                    ml-auto w-8 h-8 rounded-full flex items-center justify-center text-xs font-bold
                    ${isDarkMode ? 'bg-white/10 text-iii-light' : 'bg-black/5 text-iii-black'}
                  `}
                  >
                    {String(index + 1).padStart(2, '0')}
                  </div>
                </div>

                {/* Code */}
                <div
                  className={`flex-1 p-0 overflow-hidden text-left ${isDarkMode ? 'bg-iii-black' : 'bg-iii-light'}`}
                >
                  <Highlight
                    key={isDarkMode ? 'dark' : 'light'}
                    theme={isDarkMode ? themes.nightOwl : themes.github}
                    code={block.code}
                    language={block.language as any}
                  >
                    {({
                      className,
                      style,
                      tokens,
                      getLineProps,
                      getTokenProps,
                    }) => (
                      <pre
                        className={`text-[11px] md:text-xs leading-relaxed overflow-x-hidden whitespace-pre p-5 h-full ${className}`}
                        style={{
                          ...style,
                          background: 'transparent',
                          margin: 0,
                        }}
                      >
                        {tokens.map((line, i) => (
                          <div key={i} {...getLineProps({ line })}>
                            {line.map((token, key) => (
                              <span key={key} {...getTokenProps({ token })} />
                            ))}
                          </div>
                        ))}
                      </pre>
                    )}
                  </Highlight>
                </div>
              </div>
            </div>
          ))}
        </div>

        {/* Caption */}
        {/* <div className="mt-8 md:mt-12 text-center">
          <div
            className={`
            inline-flex items-center gap-2 sm:gap-3 px-3 sm:px-5 py-2.5 sm:py-3 rounded-lg
            ${isDarkMode ? "bg-iii-dark/40" : "bg-white/60"}
            border ${borderColor} max-w-[90vw]
          `}
          >
            <div
              className={`w-2 h-2 rounded-full flex-shrink-0 ${accentBg} animate-pulse`}
            />
            <span
              className={`text-[11px] sm:text-xs md:text-sm ${textSecondary} text-left`}
            >
              <span className={accentColor}>IPC</span>{" "}
              <span className="hidden sm:inline">
                <span className={accentColor}>
                  Inter-process communication
                </span>{" "}
              </span>
              across languages, domains, and systems
            </span>
          </div>
        </div> */}
      </div>
    </section>
  );
}
