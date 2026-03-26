import { useState } from 'react';
import { ArrowNarrowRightIcon, CheckedIcon } from './icons';

interface EmailSignupFormProps {
  isDarkMode?: boolean;
  className?: string;
  showHelperText?: boolean;
}

export function EmailSignupForm({
  isDarkMode = true,
  className = '',
  showHelperText = true,
}: EmailSignupFormProps) {
  const [email, setEmail] = useState('');
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [isSubmitted, setIsSubmitted] = useState(() => {
    if (typeof window === 'undefined') {
      return false;
    }
    try {
      return window.localStorage.getItem('iii_access_requested') === 'true';
    } catch {
      return false;
    }
  });

  const persistAccessRequest = (requestedEmail: string) => {
    if (typeof window === 'undefined') {
      return;
    }
    try {
      window.localStorage.setItem('iii_access_requested', 'true');
      window.localStorage.setItem('iii_access_email', requestedEmail);
    } catch {
      // Ignore storage failures (private mode, blocked storage, etc).
    }
  };

  const handleEmailSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!email) return;

    setIsSubmitting(true);

    try {
      const formUrl = import.meta.env.VITE_MAILMODO_FORM_URL;
      if (formUrl) {
        const res = await fetch(formUrl, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ email }),
        });
        if (!res.ok && res.status !== 409) {
          throw new Error('Submission failed');
        }
      }
      setIsSubmitted(true);
      persistAccessRequest(email);
      setEmail('');
    } catch {
      setIsSubmitted(true);
      persistAccessRequest(email);
      setEmail('');
    } finally {
      setIsSubmitting(false);
    }
  };

  return (
    <div className={`w-full sm:w-auto relative ${className}`}>
      <form onSubmit={handleEmailSubmit} className="flex items-center w-full">
        {isSubmitted ? (
          <div
            className={`flex items-center gap-2 text-xs md:text-sm px-3 py-2.5 md:px-4 md:py-3 border rounded w-full justify-center ${
              isDarkMode
                ? 'text-iii-accent bg-iii-accent/10 border-iii-accent/20'
                : 'text-iii-accent-light bg-iii-accent-light/10 border-iii-accent-light/20'
            }`}
          >
            <CheckedIcon size={16} />
            <span className="font-mono tracking-tight text-xs sm:text-sm">
              SUBSCRIBED — STAY UPDATED
            </span>
          </div>
        ) : (
          <div
            className={`flex w-full border-b transition-colors relative ${
              isDarkMode
                ? 'border-iii-light focus-within:border-iii-accent'
                : 'border-iii-dark focus-within:border-iii-accent-light'
            }`}
          >
            <input
              type="email"
              placeholder="your@email.here"
              className={`bg-transparent outline-none text-xs md:text-sm py-2.5 md:py-3 px-1 w-full sm:w-48 md:w-64 placeholder-iii-medium/50 font-mono ${
                isDarkMode ? 'text-iii-light' : 'text-iii-black'
              }`}
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              required
            />
            <button
              type="submit"
              disabled={isSubmitting}
              className={`absolute right-0 top-1/2 -translate-y-1/2 disabled:opacity-50 transition-colors p-1.5 md:p-2 ${
                isDarkMode
                  ? 'text-iii-light hover:text-iii-accent'
                  : 'text-iii-black hover:text-iii-accent-light'
              }`}
            >
              {isSubmitting ? '...' : <ArrowNarrowRightIcon size={20} />}
            </button>
          </div>
        )}
      </form>
      {showHelperText ? (
        <p className="absolute left-0 top-full mt-1 text-[10px] sm:text-xs text-iii-medium">
          Follow our development
        </p>
      ) : null}
    </div>
  );
}
