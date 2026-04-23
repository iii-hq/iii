import React, { useState, useEffect } from "react";
import { Logo } from "./Logo";
import { ModeToggle } from "./ModeToggle";
import { Menu } from "lucide-react";
import {
  CheckedIcon,
  TerminalIcon,
  BrightnessDownIcon,
  MoonIcon,
  XIcon,
} from "./icons";

interface NavbarProps {
  isDarkMode: boolean;
  isGodMode?: boolean;
  isHumanMode: boolean;
  onToggleTheme: () => void;
  onToggleMode: () => void;
}

export const Navbar: React.FC<NavbarProps> = ({
  isDarkMode,
  isGodMode = false,
  isHumanMode,
  onToggleTheme,
  onToggleMode,
}) => {
  const [isScrolled, setIsScrolled] = useState(false);
  const [installCopied, setInstallCopied] = useState(false);
  const [isMobileMenuOpen, setIsMobileMenuOpen] = useState(false);
  const [starCount, setStarCount] = useState<number | null>(null);
  const installCmd =
    "curl -fsSL https://install.iii.dev/iii/main/install.sh | sh";

  useEffect(() => {
    const handleScroll = () => {
      setIsScrolled(window.scrollY > 50);
    };
    window.addEventListener("scroll", handleScroll);
    return () => window.removeEventListener("scroll", handleScroll);
  }, []);

  useEffect(() => {
    fetch("https://api.github.com/repos/iii-hq/iii")
      .then((res) => res.json())
      .then((data) => {
        if (data.stargazers_count) setStarCount(data.stargazers_count);
      })
      .catch(() => {});
  }, []);

  useEffect(() => {
    if (isMobileMenuOpen) {
      document.body.style.overflow = "hidden";
    } else {
      document.body.style.overflow = "";
    }
    return () => {
      document.body.style.overflow = "";
    };
  }, [isMobileMenuOpen]);

  const handleInstallClick = () => {
    navigator.clipboard
      .writeText(installCmd)
      .then(() => {
        setInstallCopied(true);
        setTimeout(() => setInstallCopied(false), 3000);
      })
      .catch(() => {});
  };

  const closeMobileMenu = () => setIsMobileMenuOpen(false);

  return (
    <>
      <nav
        className={`fixed top-0 left-0 right-0 z-50 w-full px-3 sm:px-4 md:px-8 lg:px-12 flex justify-between items-center border-b backdrop-blur-lg transition-all duration-300 ${
          isScrolled ? "py-2" : "py-3 sm:py-4 md:py-6"
        } ${
          isDarkMode
            ? "border-iii-light/20 bg-iii-black/90"
            : "border-iii-dark/20 bg-iii-light/90"
        }`}
      >
        <div className="flex items-center gap-4">
          <a href="/" className="cursor-pointer">
            <Logo
              className={`transition-all duration-300 ${
                isScrolled ? "h-5 md:h-7" : "h-6 md:h-10"
              } ${
                isGodMode
                  ? "text-red-500"
                  : isDarkMode
                    ? "text-iii-light"
                    : "text-iii-black"
              }`}
              accentColor={
                isGodMode
                  ? "fill-red-500"
                  : isDarkMode
                    ? "fill-iii-accent"
                    : "fill-iii-accent-light"
              }
            />
          </a>
          <div className="hidden sm:block">
            <ModeToggle
              isHumanMode={isHumanMode}
              onToggle={onToggleMode}
              isDarkMode={isDarkMode}
            />
          </div>
        </div>

        {/* Desktop Navigation */}
        <div
          className={`hidden md:flex gap-2 md:gap-4 text-[10px] md:text-sm ${
            isDarkMode ? "text-iii-light" : "text-iii-black"
          } font-semibold tracking-tight items-center`}
        >
          <button
            onClick={handleInstallClick}
            className={`flex items-center gap-2 px-3 py-1.5 rounded border transition-all duration-300 font-mono text-xs ${
              installCopied
                ? isDarkMode
                  ? "bg-iii-accent/20 border-iii-accent text-iii-accent"
                  : "bg-iii-accent-light/20 border-iii-accent-light text-iii-accent-light"
                : isDarkMode
                  ? "border-iii-light hover:bg-iii-light hover:text-iii-black"
                  : "border-iii-dark hover:bg-iii-dark hover:text-iii-light"
            }`}
          >
            {installCopied ? (
              <>
                <CheckedIcon size={12} />
                <span className="whitespace-nowrap">{installCmd}</span>
              </>
            ) : (
              <>
                <TerminalIcon size={12} />
                <span>install.sh</span>
              </>
            )}
          </button>
          <a
            href="/manifesto"
            className={`transition-colors ${
              isDarkMode
                ? "hover:text-iii-accent"
                : "hover:text-iii-accent-light"
            }`}
          >
            MANIFESTO
          </a>
          <a
            href="/docs"
            className={`transition-colors uppercase ${
              isDarkMode
                ? "text-iii-accent hover:text-iii-light"
                : "text-iii-accent-light hover:text-iii-black"
            }`}
          >
            DOCS
          </a>
          <a
            href="https://github.com/iii-hq/iii"
            target="_blank"
            rel="noopener noreferrer"
            aria-label="GitHub repository"
            className={`flex items-center gap-1.5 px-3 py-1.5 rounded border transition-all duration-300 text-xs ${
              isDarkMode
                ? "border-iii-light text-iii-light hover:bg-iii-light hover:text-iii-black"
                : "border-iii-dark text-iii-dark hover:bg-iii-dark hover:text-iii-light"
            }`}
          >
            <svg viewBox="0 0 16 16" className="w-4 h-4" fill="currentColor">
              <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0016 8c0-4.42-3.58-8-8-8z" />
            </svg>
            {starCount !== null && (
              <span className="font-mono font-semibold">
                {starCount.toLocaleString()}
              </span>
            )}
          </a>
          <button
            onClick={onToggleTheme}
            className={`p-2 rounded-full transition-colors ${
              isDarkMode
                ? "hover:bg-iii-dark text-iii-medium-dark hover:text-iii-light"
                : "hover:bg-iii-medium-light/10 text-iii-medium-light hover:text-iii-black"
            }`}
            aria-label="Toggle theme"
          >
            {isDarkMode ? (
              <BrightnessDownIcon size={16} />
            ) : (
              <MoonIcon size={16} />
            )}
          </button>
        </div>

        {/* Mobile Navigation Controls */}
        <div className="flex md:hidden items-center gap-2">
          <a
            href="https://github.com/iii-hq/iii"
            target="_blank"
            rel="noopener noreferrer"
            aria-label="GitHub repository"
            className={`flex items-center gap-1 px-2 py-1.5 rounded border transition-all text-[10px] ${
              isDarkMode
                ? "border-iii-light/20 text-iii-light/70"
                : "border-iii-dark/20 text-iii-dark/70"
            }`}
          >
            <svg
              viewBox="0 0 16 16"
              className="w-3.5 h-3.5"
              fill="currentColor"
            >
              <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0016 8c0-4.42-3.58-8-8-8z" />
            </svg>
            {starCount !== null && (
              <span className="font-mono font-semibold">
                {starCount.toLocaleString()}
              </span>
            )}
          </a>
          <button
            onClick={onToggleTheme}
            className={`p-2 rounded-full transition-colors ${
              isDarkMode
                ? "hover:bg-iii-dark text-iii-medium-dark hover:text-iii-light"
                : "hover:bg-iii-medium-light/10 text-iii-medium-light hover:text-iii-black"
            }`}
            aria-label="Toggle theme"
          >
            {isDarkMode ? (
              <BrightnessDownIcon size={16} />
            ) : (
              <MoonIcon size={16} />
            )}
          </button>
          <button
            onClick={() => setIsMobileMenuOpen(true)}
            className={`p-2 rounded-lg transition-colors ${
              isDarkMode
                ? "hover:bg-iii-dark text-iii-light"
                : "hover:bg-iii-medium-light/10 text-iii-black"
            }`}
            aria-label="Open menu"
          >
            <Menu className="w-5 h-5" />
          </button>
        </div>
      </nav>

      {/* Mobile Menu Overlay */}
      {isMobileMenuOpen && (
        <div className="fixed inset-0 z-[60] md:hidden">
          <div
            className="absolute inset-0 bg-black/60 backdrop-blur-sm"
            onClick={closeMobileMenu}
          />
          <div
            className={`absolute top-0 right-0 h-full w-[280px] max-w-[85vw] p-6 border-l transition-transform duration-300 ${
              isDarkMode
                ? "bg-iii-black border-iii-light/20"
                : "bg-iii-light border-iii-dark/20"
            }`}
          >
            <div className="flex justify-between items-center mb-8">
              <span
                className={`text-sm font-bold tracking-wider ${
                  isDarkMode ? "text-iii-light" : "text-iii-black"
                }`}
              >
                MENU
              </span>
              <button
                onClick={closeMobileMenu}
                className={`p-2 rounded-lg transition-colors ${
                  isDarkMode
                    ? "hover:bg-iii-dark text-iii-light"
                    : "hover:bg-iii-medium-light/10 text-iii-black"
                }`}
                aria-label="Close menu"
              >
                <XIcon size={20} />
              </button>
            </div>

            <nav className="flex flex-col gap-4">
              <a
                href="/manifesto"
                onClick={closeMobileMenu}
                className={`py-3 px-4 rounded-lg border-2 text-sm font-bold transition-all ${
                  isDarkMode
                    ? "border-iii-light/10 hover:border-iii-accent hover:text-iii-accent text-iii-light"
                    : "border-iii-dark/10 hover:border-iii-accent-light hover:text-iii-accent-light text-iii-black"
                }`}
              >
                MANIFESTO
              </a>
              <a
                href="/docs"
                onClick={closeMobileMenu}
                className={`py-3 px-4 rounded-lg border-2 text-sm font-bold transition-all ${
                  isDarkMode
                    ? "border-iii-accent text-iii-accent hover:bg-iii-accent/10"
                    : "border-iii-accent-light text-iii-accent-light hover:bg-iii-accent-light/10"
                }`}
              >
                DOCS
              </a>

              <a
                href="https://github.com/iii-hq/iii"
                target="_blank"
                rel="noopener noreferrer"
                onClick={closeMobileMenu}
                className={`flex items-center justify-center gap-2 py-3 px-4 rounded-lg border-2 text-sm font-bold transition-all ${
                  isDarkMode
                    ? "border-iii-light/10 hover:border-iii-light/30 text-iii-light"
                    : "border-iii-dark/10 hover:border-iii-dark/30 text-iii-black"
                }`}
              >
                <svg
                  viewBox="0 0 16 16"
                  className="w-4 h-4"
                  fill="currentColor"
                >
                  <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0016 8c0-4.42-3.58-8-8-8z" />
                </svg>
                GitHub
                {starCount !== null ? ` ${starCount.toLocaleString()}` : ""}
              </a>

              <div className="my-4 border-t border-iii-medium/20" />

              <button
                onClick={() => {
                  handleInstallClick();
                }}
                className={`flex items-center justify-center gap-2 py-3 px-4 rounded-lg border-2 text-sm font-mono transition-all ${
                  installCopied
                    ? isDarkMode
                      ? "bg-iii-accent/20 border-iii-accent text-iii-accent"
                      : "bg-iii-accent-light/20 border-iii-accent-light text-iii-accent-light"
                    : isDarkMode
                      ? "border-iii-light text-iii-light hover:bg-iii-light hover:text-iii-black"
                      : "border-iii-dark text-iii-black hover:bg-iii-dark hover:text-iii-light"
                }`}
              >
                {installCopied ? (
                  <>
                    <CheckedIcon size={16} />
                    <span>Copied!</span>
                  </>
                ) : (
                  <>
                    <TerminalIcon size={16} />
                    <span>install.sh</span>
                  </>
                )}
              </button>

              <div className="mt-4">
                <ModeToggle
                  isHumanMode={isHumanMode}
                  onToggle={onToggleMode}
                  isDarkMode={isDarkMode}
                />
              </div>
            </nav>
          </div>
        </div>
      )}
    </>
  );
};
