import { KeyRound, Loader2 } from "lucide-react";
import { type FormEvent, useState } from "react";
import { useTranslation } from "react-i18next";

import { mfAuth } from "@/design-system/tokens";
import { describeAppError } from "@/lib/app-error";
import { recoverDatabaseKey } from "@/services/database-security-service";

interface DatabaseRecoveryScreenProps {
  /** Backend reason (why the key could not be used). */
  reason: string;
}

/**
 * Shown at startup when the encrypted database exists but its key is missing from the OS
 * credential store (or is wrong). The recovery key is verified against the database by the backend,
 * which then restarts the app. No data is created or overwritten on this screen.
 */
export function DatabaseRecoveryScreen({ reason }: DatabaseRecoveryScreenProps) {
  const { t } = useTranslation("shell");
  const [recoveryKey, setRecoveryKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleSubmit = async (event: FormEvent) => {
    event.preventDefault();
    if (!recoveryKey.trim() || busy) return;
    setBusy(true);
    setError(null);
    try {
      await recoverDatabaseKey(recoveryKey);
      // On success the backend restarts the application.
    } catch (err) {
      setError(describeAppError(err).title);
      setBusy(false);
    }
  };

  return (
    <div className={mfAuth.startupShell}>
      <KeyRound className="h-10 w-10 text-primary" aria-hidden />
      <p className="text-lg font-semibold">{t("startup.dbRecovery.title")}</p>
      <p className="max-w-md text-sm text-text-secondary">{t("startup.dbRecovery.body")}</p>
      <p className="max-w-md text-xs text-text-muted">{reason}</p>
      <form className="flex w-full max-w-md flex-col gap-3" onSubmit={(e) => void handleSubmit(e)}>
        <label htmlFor="db-recovery-key" className="text-left text-sm font-medium">
          {t("startup.dbRecovery.label")}
        </label>
        <textarea
          id="db-recovery-key"
          className="min-h-20 rounded-md border border-border bg-surface-1 p-2 font-mono text-sm"
          value={recoveryKey}
          onChange={(e) => setRecoveryKey(e.target.value)}
          placeholder="XXXX-XXXX-XXXX-…"
          autoComplete="off"
          spellCheck={false}
          disabled={busy}
          aria-invalid={error != null}
          aria-describedby={error ? "db-recovery-error" : undefined}
        />
        {error ? (
          <p id="db-recovery-error" role="alert" className="text-left text-sm text-text-danger">
            {error}
          </p>
        ) : null}
        <button type="submit" className="btn-primary" disabled={busy || !recoveryKey.trim()}>
          {busy ? <Loader2 className="mr-2 inline h-4 w-4 animate-spin" /> : null}
          {t("startup.dbRecovery.submit")}
        </button>
      </form>
      <p className="max-w-md text-xs text-text-muted">{t("startup.dbRecovery.help")}</p>
    </div>
  );
}
