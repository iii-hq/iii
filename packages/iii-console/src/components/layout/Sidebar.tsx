'use client';

import Link from 'next/link';
import { usePathname } from 'next/navigation';
import { useEffect, useState } from 'react';
import { 
  Layers, 
  Zap, 
  Terminal, 
  Settings, 
  Server,
  LayoutDashboard,
  Plug,
  GitBranch
} from 'lucide-react';
import { clsx } from 'clsx';

const NAV_ITEMS = [
  { name: 'Dashboard', href: '/', icon: LayoutDashboard },
  { name: 'Streams', href: '/streams', icon: Layers },
  { name: 'Triggers', href: '/triggers', icon: Zap },
  { name: 'Functions', href: '/handlers', icon: Server },
  { name: 'Traces', href: '/traces', icon: GitBranch },
  { name: 'Logs', href: '/logs', icon: Terminal },
  { name: 'Adapters', href: '/adapters', icon: Plug },
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

  useEffect(() => {
    const checkStatus = async () => {
      try {
        const res = await fetch('http://localhost:9001/_/api/status');
        setIsOnline(res.ok);
      } catch {
        setIsOnline(false);
      }
    };
    
    checkStatus();
    const interval = setInterval(checkStatus, 5000);
    return () => clearInterval(interval);
  }, []);

  return (
    <div className="w-56 h-screen bg-black border-r border-border flex flex-col fixed left-0 top-0">
      {}
      <div className="p-5 border-b border-border">
        <div className="flex items-center gap-3">
          <IIILogo className="w-6 h-6 text-white" />
          <div className="leading-tight">
            <span className="text-[10px] tracking-[0.15em] text-muted uppercase block">Developer</span>
            <span className="text-[10px] tracking-[0.15em] text-muted uppercase block">Console</span>
          </div>
        </div>
      </div>

      {}
      <nav className="flex-1 p-3 space-y-0.5 overflow-y-auto">
        {NAV_ITEMS.map((item) => {
          const isActive = pathname === item.href;
          const Icon = item.icon;
          
          return (
            <Link 
              key={item.href} 
              href={item.href}
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

      {}
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
          v0.0.0 • localhost:9001
        </div>
      </div>
    </div>
  );
}
