'use client';

import Link from 'next/link';
import { usePathname } from 'next/navigation';
import { useEffect, useState } from 'react';
import { 
  Layers, 
  Terminal, 
  Settings, 
  Server,
  LayoutDashboard,
  GitBranch,
  Database,
  Menu,
  X
} from 'lucide-react';
import { clsx } from 'clsx';

const NAV_ITEMS = [
  { name: 'Dashboard', href: '/', icon: LayoutDashboard },
  { name: 'States', href: '/states', icon: Database },
  { name: 'Streams', href: '/streams', icon: Layers },
  { name: 'Functions', href: '/handlers', icon: Server },
  { name: 'Traces', href: '/traces', icon: GitBranch },
  { name: 'Logs', href: '/logs', icon: Terminal },
  { name: 'Config', href: '/config', icon: Settings },
];

function IIILogo({ className = "" }: { className?: string }) {
  return (
    <svg 
      viewBox="0 0 28 27" 
      fill="currentColor" 
      className={className}
      aria-label="iii logo"
    >
      {}
      <rect x="0" y="0" width="4" height="4" />
      <rect x="12" y="0" width="4" height="4" />
      <rect x="24" y="0" width="4" height="4" />
      {}
      <rect x="0" y="7" width="4" height="20" />
      <rect x="12" y="7" width="4" height="20" />
      <rect x="24" y="7" width="4" height="20" />
    </svg>
  );
}

export function Sidebar() {
  const pathname = usePathname();
  const [isOnline, setIsOnline] = useState(false);
  const [isMobileMenuOpen, setIsMobileMenuOpen] = useState(false);

  useEffect(() => {
    const checkStatus = async () => {
      try {
        const res = await fetch('http://localhost:3111/_console/health');
        setIsOnline(res.ok);
      } catch {
        setIsOnline(false);
      }
    };
    
    checkStatus();
    const interval = setInterval(checkStatus, 5000);
    return () => clearInterval(interval);
  }, []);

  // Close mobile menu on route change
  useEffect(() => {
    setIsMobileMenuOpen(false);
  }, [pathname]);

  // Close mobile menu on escape key
  useEffect(() => {
    const handleEscape = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setIsMobileMenuOpen(false);
    };
    document.addEventListener('keydown', handleEscape);
    return () => document.removeEventListener('keydown', handleEscape);
  }, []);

  // Prevent body scroll when mobile menu is open
  useEffect(() => {
    if (isMobileMenuOpen) {
      document.body.style.overflow = 'hidden';
    } else {
      document.body.style.overflow = '';
    }
    return () => { document.body.style.overflow = ''; };
  }, [isMobileMenuOpen]);

  const sidebarContent = (
    <>
      {/* Logo */}
      <div className="p-5 border-b border-border flex items-center justify-between">
        <div className="flex items-center gap-3">
          <IIILogo className="w-6 h-6 text-white" />
          <div className="leading-tight">
            <span className="text-[10px] tracking-[0.15em] text-muted uppercase block">Developer</span>
            <span className="text-[10px] tracking-[0.15em] text-muted uppercase block">Console</span>
          </div>
        </div>
        {/* Close button for mobile */}
        <button 
          onClick={() => setIsMobileMenuOpen(false)}
          className="lg:hidden p-2 rounded-lg hover:bg-dark-gray transition-colors"
          aria-label="Close menu"
        >
          <X className="w-5 h-5" />
        </button>
      </div>

      {/* Navigation */}
      <nav className="flex-1 p-3 space-y-0.5 overflow-y-auto">
        {NAV_ITEMS.map((item) => {
          const isActive = pathname === item.href;
          const Icon = item.icon;
          
          return (
            <Link 
              key={item.href} 
              href={item.href}
              onClick={() => setIsMobileMenuOpen(false)}
              className={clsx(
                "flex items-center gap-3 px-3 py-2.5 text-xs tracking-wide rounded transition-all duration-150",
                isActive 
                  ? "bg-white text-black font-medium" 
                  : "text-muted hover:text-foreground hover:bg-[#1D1D1D]"
              )}
            >
              <Icon className="w-4 h-4 flex-shrink-0" strokeWidth={1.5} />
              <span className="uppercase tracking-wider">{item.name}</span>
            </Link>
          );
        })}
      </nav>

      {/* Status Footer */}
      <div className="p-4 border-t border-border">
        <div className="flex items-center gap-2">
          <div className={clsx(
            "flex items-center gap-2 px-3 py-1.5 rounded-full border",
            isOnline 
              ? "border-[#22C55E]/50" 
              : "border-[#EF4444]/50"
          )}>
            <div className={clsx(
              "w-2 h-2 rounded-full",
              isOnline 
                ? "bg-[#22C55E] shadow-[0_0_6px_#22C55E]" 
                : "bg-[#EF4444] shadow-[0_0_6px_#EF4444]"
            )} />
            <span className="text-[10px] tracking-[0.1em] text-foreground uppercase">
              {isOnline ? 'Online' : 'Offline'}
            </span>
          </div>
        </div>
        <div className="mt-3 text-[9px] text-muted/60 tracking-wide font-mono">
          v0.0.0 • localhost:3111
        </div>
      </div>
    </>
  );

  return (
    <>
      {/* Mobile Header Bar */}
      <div className="lg:hidden fixed top-0 left-0 right-0 z-40 h-14 bg-black border-b border-border flex items-center justify-between px-4">
        <div className="flex items-center gap-3">
          <IIILogo className="w-5 h-5 text-white" />
          <span className="text-xs tracking-[0.15em] text-muted uppercase">iii Console</span>
        </div>
        <div className="flex items-center gap-3">
          {/* Status indicator */}
          <div className={clsx(
            "w-2 h-2 rounded-full",
            isOnline 
              ? "bg-[#22C55E] shadow-[0_0_6px_#22C55E]" 
              : "bg-[#EF4444] shadow-[0_0_6px_#EF4444]"
          )} />
          {/* Hamburger button */}
          <button 
            onClick={() => setIsMobileMenuOpen(true)}
            className="p-2 rounded-lg hover:bg-dark-gray transition-colors"
            aria-label="Open menu"
          >
            <Menu className="w-5 h-5" />
          </button>
        </div>
      </div>

      {/* Mobile Overlay */}
      {isMobileMenuOpen && (
        <div 
          className="lg:hidden fixed inset-0 z-40 bg-black/60 backdrop-blur-sm"
          onClick={() => setIsMobileMenuOpen(false)}
        />
      )}

      {/* Sidebar - Desktop: always visible, Mobile: slide-in drawer */}
      <div className={clsx(
        "w-56 h-screen bg-black border-r border-border flex flex-col fixed left-0 top-0 z-50 transition-transform duration-300 ease-in-out",
        // Mobile: slide in/out
        "lg:translate-x-0",
        isMobileMenuOpen ? "translate-x-0" : "-translate-x-full lg:translate-x-0"
      )}>
        {sidebarContent}
      </div>
    </>
  );
}
