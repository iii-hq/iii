import React from 'react';

interface CookieConsentProps {
  onAccept: () => void;
  onReject: () => void;
}

export const CookieConsent: React.FC<CookieConsentProps> = ({
  onAccept,
  onReject,
}) => {
  return (
    <div className="fixed bottom-0 inset-x-0 z-50 p-4 sm:p-6 pointer-events-none">
      <div className="max-w-xl mx-auto bg-iii-dark border border-iii-medium rounded-lg shadow-2xl p-4 sm:p-5 font-mono pointer-events-auto animate-fade-in-up">
        <p className="text-sm text-iii-medium-light mb-4">
          This site uses cookies to understand how visitors find us.{' '}
          <span className="text-iii-light">
            You can accept or decline non-essential cookies.
          </span>
        </p>
        <div className="flex gap-3 justify-end">
          <button
            onClick={onReject}
            className="px-4 py-1.5 text-xs rounded border border-iii-medium text-iii-medium-light hover:text-iii-light hover:border-iii-medium-light transition-colors cursor-pointer"
          >
            Decline
          </button>
          <button
            onClick={onAccept}
            className="px-4 py-1.5 text-xs rounded bg-iii-accent text-iii-black font-semibold hover:brightness-110 transition-all cursor-pointer"
          >
            Accept
          </button>
        </div>
      </div>
    </div>
  );
};
