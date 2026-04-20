import React, { useEffect, useState } from 'react';
import { Navbar } from '../components/Navbar';
import { Terminal } from '../components/Terminal';
import { CookieConsent } from '../components/CookieConsent';
import { useCookieConsent } from '../lib/useCookieConsent';
import { FooterSection } from '../components/sections/FooterSection';
import { HeroV3 } from '../components/sections-v3/HeroV3';
import { OneModelV3 } from '../components/sections-v3/OneModelV3';
import { BackendV3 } from '../components/sections-v3/BackendV3';
import { AgentsV3 } from '../components/sections-v3/AgentsV3';
import { PlatformV3 } from '../components/sections-v3/PlatformV3';
import { RegistryV3 } from '../components/sections-v3/RegistryV3';
import { ConsoleV3 } from '../components/sections-v3/ConsoleV3';
import { GetStartedV3 } from '../components/sections-v3/GetStartedV3';
import { KeySequence } from '../types';

export const HomeV3: React.FC = () => {
  const { consent, accept, reject } = useCookieConsent();
  const [isDarkMode, setIsDarkMode] = useState(true);
  const [showTerminal, setShowTerminal] = useState(false);
  const [isGodMode, setIsGodMode] = useState(false);
  const [showGodModeUnlock, setShowGodModeUnlock] = useState(false);

  useEffect(() => {
    document.body.classList.toggle('dark-mode', isDarkMode);
    document.body.classList.toggle('light-mode', !isDarkMode);
  }, [isDarkMode]);

  const toggleTheme = () => setIsDarkMode((v) => !v);

  const toggleMode = () => {
    window.history.pushState({}, '', '/ai');
    window.location.reload();
  };

  useEffect(() => {
    let keyBuffer: string[] = [];

    const handleKeyDown = (e: KeyboardEvent) => {
      const key = e.key.length === 1 ? e.key.toLowerCase() : e.key;
      keyBuffer.push(key);
      if (keyBuffer.length > 30) keyBuffer = keyBuffer.slice(-30);

      const recentKeys = keyBuffer.slice(-3).join('');
      if (recentKeys === KeySequence.III) setShowTerminal(true);

      const fullHistory = keyBuffer.join('');
      if (fullHistory.includes(KeySequence.KONAMI)) {
        setIsGodMode(true);
        setShowGodModeUnlock(true);
        setTimeout(() => {
          setShowGodModeUnlock(false);
          setShowTerminal(true);
        }, 2000);
        keyBuffer = [];
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, []);

  return (
    <div
      className={`min-h-screen font-mono selection:bg-iii-accent selection:text-iii-black relative flex flex-col transition-colors duration-300 ${
        isDarkMode
          ? 'bg-iii-black text-iii-light'
          : 'bg-iii-light text-iii-black'
      } ${isGodMode ? 'selection:bg-red-500' : ''}`}
    >
      <Navbar
        isDarkMode={isDarkMode}
        isGodMode={isGodMode}
        isHumanMode={true}
        onToggleTheme={toggleTheme}
        onToggleMode={toggleMode}
      />

      <main className="flex-1 relative z-10 flex flex-col w-full pt-16 md:pt-20">
        <HeroV3 isDarkMode={isDarkMode} />
        <OneModelV3 isDarkMode={isDarkMode} />
        <BackendV3 isDarkMode={isDarkMode} />
        <AgentsV3 isDarkMode={isDarkMode} />
        <PlatformV3 isDarkMode={isDarkMode} />
        <RegistryV3 isDarkMode={isDarkMode} />
        <ConsoleV3 isDarkMode={isDarkMode} />
        <GetStartedV3 isDarkMode={isDarkMode} />
        <FooterSection isDarkMode={isDarkMode} />
      </main>

      {showGodModeUnlock && (
        <div className="fixed inset-0 z-50 bg-black flex items-center justify-center">
          <div className="text-center animate-pulse">
            <div className="text-6xl md:text-8xl font-black text-red-500 mb-4 tracking-tighter animate-bounce">
              GOD MODE
            </div>
            <div className="text-xl md:text-2xl text-red-400 font-mono tracking-widest">
              UNLOCKED
            </div>
            <div className="mt-8 text-sm text-red-500/50 font-mono">
              ↑↑↓↓←→←→BA
            </div>
          </div>
        </div>
      )}

      {showTerminal && (
        <Terminal
          onClose={() => setShowTerminal(false)}
          isGodMode={isGodMode}
        />
      )}

      {consent === null && (
        <CookieConsent onAccept={accept} onReject={reject} />
      )}
    </div>
  );
};
