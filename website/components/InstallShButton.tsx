import { useState } from "react";
import { CheckedIcon, CopyIcon, TerminalIcon } from "./icons";

const INSTALL_COMMAND = "curl -fsSL https://install.iii.dev/iii/main/install.sh | sh";

interface InstallShButtonProps {
  isDarkMode?: boolean;
  className?: string;
}

export function InstallShButton({
  isDarkMode = true,
  className = "",
}: InstallShButtonProps) {
  const [copySuccess, setCopySuccess] = useState(false);

  const copyToClipboard = () => {
    navigator.clipboard.writeText(INSTALL_COMMAND);
    setCopySuccess(true);
    setTimeout(() => setCopySuccess(false), 2000);
  };

  return (
    <div className={`flex flex-col gap-1.5 w-full sm:w-auto ${className}`}>
      <div
        className={`group relative flex items-center gap-2 px-2.5 py-2 sm:px-3 sm:py-2.5 md:px-4 md:py-3 border rounded transition-colors cursor-pointer w-full sm:min-w-[220px] md:min-w-[280px] ${
          isDarkMode
            ? "bg-iii-dark/50 border-iii-light hover:border-iii-light"
            : "bg-white/50 border-iii-dark hover:border-iii-dark"
        }`}
        onClick={copyToClipboard}
      >
        {copySuccess ? (
          <>
            <CheckedIcon
              size={16}
              className={`flex-shrink-0 ${
                isDarkMode ? "text-iii-accent" : "text-iii-accent-light"
              }`}
            />
            <code
              className={`text-[9px] sm:text-[10px] md:text-sm flex-1 truncate ${
                isDarkMode ? "text-iii-light" : "text-iii-black"
              }`}
            >
              copied!
            </code>
          </>
        ) : (
          <>
            <TerminalIcon
              size={16}
              className={`transition-colors flex-shrink-0 ${
                isDarkMode
                  ? "text-iii-light/50 group-hover:text-iii-accent"
                  : "text-iii-dark/50 group-hover:text-iii-accent-light"
              }`}
            />
            <code
              className={`text-[9px] sm:text-[10px] md:text-sm flex-1 truncate ${
                isDarkMode ? "text-iii-light" : "text-iii-black"
              }`}
            >
              install.sh
            </code>
            <CopyIcon
              size={16}
              className={`transition-colors flex-shrink-0 ${
                isDarkMode
                  ? "text-iii-light/50 group-hover:text-white"
                  : "text-iii-dark/50 group-hover:text-iii-black"
              }`}
            />
          </>
        )}
      </div>
    </div>
  );
}
