import { type FormEvent, useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

interface LockScreenProps {
  displayName: string | null;
  onUnlock: (password: string) => Promise<void>;
  onUnlockWithPin?: (pin: string) => Promise<void>;
  onLogout: () => void;
  pinConfigured?: boolean;
}

const MAX_PIN_FAILURES = 3;

export function LockScreen({
  displayName,
  onUnlock,
  onUnlockWithPin,
  onLogout,
  pinConfigured = false,
}: LockScreenProps) {
  const { t } = useTranslation("auth");
  const [mode, setMode] = useState<"pin" | "password">(
    pinConfigured && onUnlockWithPin ? "pin" : "password",
  );
  const [password, setPassword] = useState("");
  const [pinDigits, setPinDigits] = useState<string[]>([]);
  const [pinLength] = useState(4); // default PIN length
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [pinFailures, setPinFailures] = useState(0);
  const pinRefs = useRef<(HTMLInputElement | null)[]>([]);

  // Force password mode after too many PIN failures
  const pinDisabled = pinFailures >= MAX_PIN_FAILURES;

  useEffect(() => {
    if (pinDisabled && mode === "pin") {
      setMode("password");
      setError(
        t("session.pin.tooManyAttempts", "Trop de tentatives. Utilisez votre mot de passe."),
      );
    }
  }, [pinDisabled, mode, t]);

  // Auto-focus first PIN input on mode change
  useEffect(() => {
    if (mode === "pin") {
      setPinDigits([]);
      setTimeout(() => pinRefs.current[0]?.focus(), 50);
    }
  }, [mode]);

  // ── PIN submit ───────────────────────────────────────────────────────

  const submitPin = useCallback(
    async (pin: string) => {
      if (!onUnlockWithPin || loading) return;
      setLoading(true);
      setError(null);
      try {
        await onUnlockWithPin(pin);
      } catch (err) {
        setPinFailures((f) => f + 1);
        setPinDigits([]);
        pinRefs.current[0]?.focus();
        setError(err instanceof Error ? err.message : t("session.pin.invalid", "PIN incorrect."));
      } finally {
        setLoading(false);
      }
    },
    [onUnlockWithPin, loading, t],
  );

  // ── PIN input handling ──────────────────────────────────────────────────

  const handlePinInput = useCallback(
    (index: number, value: string) => {
      if (!/^\d?$/.test(value)) return;

      const newDigits = [...pinDigits];
      newDigits[index] = value;
      setPinDigits(newDigits);
      setError(null);

      if (value && index < pinLength - 1) {
        pinRefs.current[index + 1]?.focus();
      }

      // Auto-submit when all digits filled
      const fullPin = newDigits.join("");
      if (fullPin.length === pinLength && newDigits.every(Boolean)) {
        void submitPin(fullPin);
      }
    },
    [pinDigits, pinLength, submitPin],
  );

  const handlePinKeyDown = useCallback(
    (index: number, e: React.KeyboardEvent) => {
      if (e.key === "Backspace" && !pinDigits[index] && index > 0) {
        pinRefs.current[index - 1]?.focus();
      }
    },
    [pinDigits],
  );

  // ── Password submit ─────────────────────────────────────────────────────

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    setLoading(true);
    try {
      await onUnlock(password);
    } catch (err) {
      setError(err instanceof Error ? err.message : t("session.idleLocked.unlockAction"));
      setPassword("");
    } finally {
      setLoading(false);
    }
  };

  // ── Toggle mode ─────────────────────────────────────────────────────────

  const canToggle = pinConfigured && onUnlockWithPin && !pinDisabled;

  return (
    <div className="flex min-h-screen items-center justify-center bg-surface-0 px-4">
      <div className="w-full max-w-sm text-center">
        {/* User avatar */}
        <div
          className="mx-auto mb-4 flex h-16 w-16 items-center justify-center
                     rounded-full bg-primary text-2xl font-bold text-white"
        >
          {displayName ? displayName.charAt(0).toUpperCase() : "?"}
        </div>

        <h2 className="text-lg font-semibold text-text-primary">{t("session.idleLocked.title")}</h2>
        <p className="mt-1 text-sm text-text-secondary">{t("session.idleLocked.message")}</p>

        {displayName && <p className="mt-2 text-sm font-medium text-text-primary">{displayName}</p>}

        {/* PIN mode */}
        {mode === "pin" && (
          <div className="mt-6 space-y-4">
            <p className="text-sm text-text-secondary">
              {t("session.pin.enterPin", "Entrez votre PIN")}
            </p>
            <div className="flex justify-center gap-2">
              {Array.from({ length: pinLength }).map((_, i) => (
                <input
                  // biome-ignore lint/suspicious/noArrayIndexKey: fixed-length PIN digit array
                  key={`pin-${i}`}
                  ref={(el) => {
                    pinRefs.current[i] = el;
                  }}
                  type="text"
                  inputMode="numeric"
                  maxLength={1}
                  value={pinDigits[i] ?? ""}
                  onChange={(e) => handlePinInput(i, e.target.value)}
                  onKeyDown={(e) => handlePinKeyDown(i, e)}
                  disabled={loading}
                  className="h-12 w-12 rounded-md border border-surface-border bg-surface-1
                             text-center text-lg font-semibold text-text-primary
                             focus:border-primary focus:outline-none focus:ring-1 focus:ring-primary
                             disabled:opacity-50"
                  // biome-ignore lint/a11y/noAutofocus: lock screen must focus first PIN input
                  autoFocus={i === 0}
                />
              ))}
            </div>
            {loading && (
              <div className="flex justify-center">
                <div className="h-4 w-4 animate-spin rounded-full border-2 border-surface-3 border-t-primary" />
              </div>
            )}
          </div>
        )}

        {/* Password mode */}
        {mode === "password" && (
          <form onSubmit={handleSubmit} className="mt-6 space-y-4">
            <div>
              <label htmlFor="lock-password" className="sr-only">
                {t("session.idleLocked.unlockPrompt")}
              </label>
              <input
                id="lock-password"
                type="password"
                autoComplete="current-password"
                // biome-ignore lint/a11y/noAutofocus: lock screen must focus password input
                autoFocus
                required
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                placeholder={t("session.idleLocked.unlockPrompt")}
                className="w-full rounded-md border border-surface-border bg-surface-1
                           px-3 py-2 text-sm text-text-primary text-center
                           placeholder:text-text-muted
                           focus:border-primary focus:outline-none focus:ring-1
                           focus:ring-primary"
                disabled={loading}
              />
            </div>

            <button
              type="submit"
              disabled={loading}
              className="btn-primary w-full py-2 text-sm font-medium"
            >
              {loading ? t("session.idleLocked.unlocking") : t("session.idleLocked.unlockAction")}
            </button>
          </form>
        )}

        {/* Error display */}
        {error && (
          <div
            role="alert"
            className="mt-4 rounded-md bg-status-danger/10 px-3 py-2 text-sm text-status-danger"
          >
            {error}
          </div>
        )}

        {/* Mode toggle */}
        {canToggle && (
          <button
            type="button"
            onClick={() => {
              setMode((m) => (m === "pin" ? "password" : "pin"));
              setError(null);
            }}
            className="mt-4 text-xs text-primary hover:text-primary/80 transition-colors"
          >
            {mode === "pin"
              ? t("session.pin.usePassword", "Utiliser le mot de passe")
              : t("session.pin.usePin", "Utiliser le PIN")}
          </button>
        )}

        {/* Sign out link */}
        <button
          type="button"
          onClick={onLogout}
          className="mt-4 block w-full text-xs text-text-muted hover:text-text-secondary
                     transition-colors"
        >
          {t("logout.label")}
        </button>
      </div>
    </div>
  );
}
