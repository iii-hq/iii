import { useState, useEffect, useCallback } from "react";

type ConsentState = "accepted" | "rejected" | null;

const STORAGE_KEY = "iii_cookie_consent";

function loadCommonRoom() {
  if (typeof window === "undefined") return;
  if (typeof (window as any).signals !== "undefined") return;

  const script = document.createElement("script");
  script.src =
    "https://cdn.cr-relay.com/v1/site/da18833a-8f00-4ad0-9833-6608b59a713a/signals.js";
  script.async = true;

  (window as any).signals = Object.assign(
    [],
    { _opts: { apiHost: "https://api.cr-relay.com" } },
    ["page", "identify", "form"].reduce(
      (acc: Record<string, Function>, method: string) => {
        acc[method] = function () {
          (window as any).signals.push([method, arguments]);
          return (window as any).signals;
        };
        return acc;
      },
      {},
    ),
  );

  document.head.appendChild(script);
}

export function useCookieConsent() {
  const [consent, setConsent] = useState<ConsentState>(() => {
    const stored = localStorage.getItem(STORAGE_KEY);
    return stored === "accepted" || stored === "rejected" ? stored : null;
  });

  useEffect(() => {
    if (consent === "accepted") loadCommonRoom();
  }, [consent]);

  const accept = useCallback(() => {
    localStorage.setItem(STORAGE_KEY, "accepted");
    setConsent("accepted");
  }, []);

  const reject = useCallback(() => {
    localStorage.setItem(STORAGE_KEY, "rejected");
    setConsent("rejected");
  }, []);

  return { consent, accept, reject } as const;
}
