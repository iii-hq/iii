import React from "react";
import { ShieldAlert } from "lucide-react";
import { XIcon, LockIcon } from "./icons";

interface LicenseModalProps {
  onClose: () => void;
}

export const LicenseModal: React.FC<LicenseModalProps> = ({ onClose }) => {
  return (
    <div
      className="fixed inset-0 z-[60] flex items-center justify-center bg-iii-black/90 backdrop-blur-md p-4 animate-fade-in"
      onClick={onClose}
    >
      <div
        className="w-full max-w-lg bg-iii-dark border border-iii-accent/30 shadow-2xl relative overflow-hidden"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="bg-iii-accent/10 px-6 py-4 border-b border-iii-accent/20 flex items-center justify-between">
          <div className="flex items-center gap-3 text-iii-accent">
            <ShieldAlert className="w-5 h-5 animate-pulse" />
            <h2 className="font-mono font-bold tracking-wider">
              PROTOCOL SPECIFICATION
            </h2>
          </div>
          <button
            onClick={onClose}
            className="text-iii-medium hover:text-iii-light transition-colors"
          >
            <XIcon size={20} />
          </button>
        </div>

        <div className="p-8 space-y-6 relative">
          <div className="absolute inset-0 flex items-center justify-center opacity-5 pointer-events-none select-none overflow-hidden">
            <span className="text-[120px] font-black text-red-500 -rotate-12 whitespace-nowrap">
              NO LICENSE
            </span>
          </div>

          <div className="space-y-4 font-mono text-sm leading-relaxed text-iii-light/80">
            <div className="flex items-start gap-3 p-3 bg-red-900/20 border border-red-500/30 rounded">
              <LockIcon size={20} className="text-red-400 shrink-0 mt-0.5" />
              <p className="text-red-300">WARNING: EXCLUDED MATERIAL.</p>
            </div>

            <p>
              This specification explicitly grants{" "}
              <span className="text-white font-bold bg-red-500/20 px-1 rounded-sm">
                NO PATENT RIGHTS
              </span>
              . Reading it describes the invention, but gives absolutely no
              legal permission to implement the backend logic that executes it.
            </p>

            <div className="border-l-2 border-iii-medium pl-4 py-1 my-4">
              <h3 className="text-white font-bold mb-1">
                The "Look but Don't Touch" Shield
              </h3>
              <p className="text-xs text-iii-medium">
                Prevents competitors from claiming "Implied License." If you
                build a backend based on this spec, you are knowingly
                infringing.
              </p>
            </div>

            <div className="pt-4 flex justify-end">
              <button
                onClick={onClose}
                className="text-xs font-bold border border-iii-medium px-4 py-2 hover:bg-iii-accent hover:text-iii-black hover:border-iii-accent transition-all"
              >
                ACKNOWLEDGE & EXIT
              </button>
            </div>
          </div>
        </div>

        <div className="h-1 w-full bg-gradient-to-r from-transparent via-iii-accent to-transparent absolute bottom-0 animate-pulse" />
      </div>
    </div>
  );
};
