import { Copy, KeyRound, Loader2 } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { useStepUp } from "@/hooks/use-step-up";
import { getDatabaseRecoveryKey } from "@/services/database-security-service";

interface DatabaseRecoveryKeyPanelProps {
  onToast: (msg: {
    title: string;
    description?: string;
    variant?: "default" | "destructive" | "success";
  }) => void;
}

/**
 * Reveals the database recovery key (admin + step-up) so it can be stored offline. Without it, an
 * encrypted database cannot be reopened if the OS credential store loses the key.
 */
export function DatabaseRecoveryKeyPanel({ onToast }: DatabaseRecoveryKeyPanelProps) {
  const { t } = useTranslation("settings");
  const { withStepUp, StepUpDialogElement } = useStepUp();
  const [recoveryKey, setRecoveryKey] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const handleReveal = async () => {
    setBusy(true);
    try {
      const key = await withStepUp(() => getDatabaseRecoveryKey());
      setRecoveryKey(key);
    } catch (err) {
      onToast({
        title: t("dbEncryption.revealError"),
        description: String(err),
        variant: "destructive",
      });
    } finally {
      setBusy(false);
    }
  };

  const handleCopy = async () => {
    if (!recoveryKey) return;
    try {
      await navigator.clipboard.writeText(recoveryKey);
      onToast({ title: t("dbEncryption.copied"), variant: "success" });
    } catch (err) {
      onToast({
        title: t("dbEncryption.copyError"),
        description: String(err),
        variant: "destructive",
      });
    }
  };

  return (
    <div className="space-y-4 rounded-lg border border-border p-4">
      <div>
        <h3 className="text-base font-semibold">{t("dbEncryption.title")}</h3>
        <p className="text-sm text-text-muted">{t("dbEncryption.description")}</p>
      </div>

      {recoveryKey ? (
        <div className="space-y-2">
          <p className="text-sm font-medium text-text-danger">{t("dbEncryption.warning")}</p>
          <code
            className="block break-all rounded-md bg-surface-2 p-3 font-mono text-sm"
            aria-label={t("dbEncryption.keyLabel")}
          >
            {recoveryKey}
          </code>
          <div className="flex flex-wrap gap-2">
            <Button type="button" variant="outline" onClick={() => void handleCopy()}>
              <Copy className="mr-2 h-4 w-4" />
              {t("dbEncryption.copy")}
            </Button>
            <Button type="button" variant="ghost" onClick={() => setRecoveryKey(null)}>
              {t("dbEncryption.hide")}
            </Button>
          </div>
        </div>
      ) : (
        <Button type="button" onClick={() => void handleReveal()} disabled={busy}>
          {busy ? (
            <Loader2 className="mr-2 h-4 w-4 animate-spin" />
          ) : (
            <KeyRound className="mr-2 h-4 w-4" />
          )}
          {t("dbEncryption.reveal")}
        </Button>
      )}

      {StepUpDialogElement}
    </div>
  );
}
