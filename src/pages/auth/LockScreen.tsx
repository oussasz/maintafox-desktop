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
const MIN_PIN_LENGTH = 4;
const MAX_PIN_LENGTH = 6;

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
  const [pinValue, setPinValue] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [pinFailures, setPinFailures] = useState(0);
  const pinInputRef = useRef<HTMLInputElement | null>(null);
  const pinSubmitTimerRef = useRef<number | null>(null);

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

  // Auto-focus PIN input on mode change
  useEffect(() => {
    if (mode === "pin") {
      setPinValue("");
      setTimeout(() => pinInputRef.current?.focus(), 50);
    }
  }, [mode]);

  useEffect(() => {
    return () => {
      if (pinSubmitTimerRef.current !== null) {
        window.clearTimeout(pinSubmitTimerRef.current);
      }
    };
  }, []);

  // ── PIN submit ───────────────────────────────────────────────────────

  const submitPin = useCallback(
    async (pin: string) => {
      if (!onUnlockWithPin || loading) return;
      if (pin.length < MIN_PIN_LENGTH || pin.length > MAX_PIN_LENGTH) return;

      if (pinSubmitTimerRef.current !== null) {
        window.clearTimeout(pinSubmitTimerRef.current);
        pinSubmitTimerRef.current = null;
      }

      setLoading(true);
      setError(null);
      try {
        await onUnlockWithPin(pin);
      } catch (err) {
        const message =
          err instanceof Error ? err.message : t("session.pin.invalid", "PIN incorrect.");

        if (message.toLowerCase().includes("pin désactivé") || message.toLowerCase().includes("pin disabled")) {
          setPinFailures(MAX_PIN_FAILURES);
        } else {
          setPinFailures((f) => f + 1);
        }

        setPinValue("");
        pinInputRef.current?.focus();
        setError(message);
      } finally {
        setLoading(false);
      }
    },
    [onUnlockWithPin, loading, t],
  );

  // ── PIN input handling ──────────────────────────────────────────────────

  useEffect(() => {
    if (mode !== "pin" || !onUnlockWithPin || loading || pinDisabled) return;
    if (pinValue.length < MIN_PIN_LENGTH || pinValue.length > MAX_PIN_LENGTH) return;

    if (pinSubmitTimerRef.current !== null) {
      window.clearTimeout(pinSubmitTimerRef.current);
      pinSubmitTimerRef.current = null;
    }

    if (pinValue.length === MAX_PIN_LENGTH) {
      void submitPin(pinValue);
      return;
    }

    // Debounce auto-submit so users can enter 5-6 digit PINs without
    // prematurely submitting at 4 digits.
    pinSubmitTimerRef.current = window.setTimeout(() => {
      void submitPin(pinValue);
    }, 450);

    return () => {
      if (pinSubmitTimerRef.current !== null) {
        window.clearTimeout(pinSubmitTimerRef.current);
        pinSubmitTimerRef.current = null;
      }
    };
  }, [mode, onUnlockWithPin, loading, pinDisabled, pinValue, submitPin]);

  const handlePinInput = useCallback(
    (value: string) => {
      const digits = value.replace(/\D/g, "").slice(0, MAX_PIN_LENGTH);
      setPinValue(digits);
      setError(null);
    },
    [],
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
            <input
              ref={pinInputRef}
              type="text"
              inputMode="numeric"
              pattern="\d*"
              autoComplete="one-time-code"
              maxLength={MAX_PIN_LENGTH}
              value={pinValue}
              onChange={(e) => handlePinInput(e.target.value)}
              disabled={loading || pinDisabled}
              className="sr-only"
              aria-label={t("session.pin.enterPin", "Entrez votre PIN")}
            />
            <div
              role="button"
              tabIndex={0}
              onClick={() => pinInputRef.current?.focus()}
              onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === " ") {
                  e.preventDefault();
                  pinInputRef.current?.focus();
                }
              }}
              className="flex justify-center gap-2"
              aria-label={t("session.pin.enterPin", "Entrez votre PIN")}
            >
              {Array.from({ length: MAX_PIN_LENGTH }).map((_, i) => (
                <div
                  // biome-ignore lint/suspicious/noArrayIndexKey: fixed-length visual PIN cells
                  key={`pin-cell-${i}`}
                  className="flex h-12 w-12 items-center justify-center rounded-md border border-surface-border bg-surface-1 text-lg font-semibold text-text-primary"
                >
                  {pinValue[i] ? "•" : ""}
                </div>
              ))}
            </div>
            <p className="text-xs text-text-muted">
              {t("session.pin.hint", "PIN à 4-6 chiffres. Saisie auto-détectée.")}
            </p>
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
