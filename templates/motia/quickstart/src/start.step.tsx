import { useState } from "react";
import { NoopNode, type NoopNodeProps, useQuickstart } from "motia/workbench";

export const Node: React.FC<NoopNodeProps> = (props) => {
  const { start } = useQuickstart();
  const [clicked, setClicked] = useState(false);

  const handleStart = () => {
    setClicked(true);
    start();
  };

  return (
    <NoopNode {...props}>
      <div className="flex flex-col gap-4 p-3 min-w-[320px]">
        <button
          onClick={handleStart}
          className="relative px-6 py-3 rounded-lg font-semibold text-white transition-all hover:scale-105 active:scale-95"
          style={{
            color: "#ffffff",
            backgroundColor: "#2862fe",
            boxShadow: "0 0 30px rgba(40, 98, 254, 0.7)",
            animation: clicked ? "none" : "pulse-glow 2s ease-in-out infinite",
          }}
        >
          <style>{`
            @keyframes pulse-glow {
              0%, 100% { box-shadow: 0 0 30px rgba(40, 98, 254, 0.7); }
              50% { box-shadow: 0 0 50px rgba(40, 98, 254, 1), 0 0 80px rgba(40, 98, 254, 0.6); }
            }

          `}</style>
          ⚡ Start the Tutorial
        </button>
      </div>
    </NoopNode>
  );
};
