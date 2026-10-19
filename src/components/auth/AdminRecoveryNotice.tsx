// Persistent notice to each administrator after an administrator was created with a vendor
// recovery code (`auth::recovery_code`). Shown after every sign-in, on every PC of the plant, until
// that administrator acknowledges it; the acknowledgement is per administrator and audited by the
// backend. It names the new account by its display name AND its username (the username is what
// signs in; a display name alone could pose as an existing administrator).

import { ShieldAlert } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { mfTone } from "@/design-system/tone";
import { appErrorText } from "@/lib/app-error";
import { cn } from "@/lib/utils";
import {
  type AdminRecoveryNotice as Notice,
  acknowledgeAdminRecoveryNotice,
  getAdminRecoveryNotices,
} from "@/services/admin-recovery-service";
import { formatDateTime } from "@/utils/format-date";

interface AdminRecoveryNoticeProps {
  /** Re-read when the signed-in account changes (its user id, or null when signed out). */
  sessionUserId: number | null;
  className?: string;
}

export function AdminRecoveryNotice({ sessionUserId, className }: AdminRecoveryNoticeProps) {
  const { t, i18n } = useTranslation("auth");
  const [notices, setNotices] = useState<Notice[]>([]);
  const [busyId, setBusyId] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async (isCancelled: () => boolean) => {
    try {
      const next = await getAdminRecoveryNotices();
      if (!isCancelled()) setNotices(next);
    } catch {
      // No answer: nothing to state; the notice stays stored and comes back on the next read.
      if (!isCancelled()) setNotices([]);
    }
  }, []);

  useEffect(() => {
    setError(null);
    if (sessionUserId === null) {
      setNotices([]);
      return;
    }
    let cancelled = false;
    void load(() => cancelled);
    return () => {
      cancelled = true;
    };
  }, [sessionUserId, load]);

  const acknowledge = async (id: number) => {
    setBusyId(id);
    setError(null);
    try {
      await acknowledgeAdminRecoveryNotice(id);
      setNotices((current) => current.filter((n) => n.id !== id));
    } catch (err) {
      setError(appErrorText(err));
    } finally {
      setBusyId(null);
    }
  };

  if (notices.length === 0) return null;

  return (
    <div className={cn("space-y-2", className)} data-testid="admin-recovery-notices">
      {notices.map((notice) => (
        <div
          key={notice.id}
          role="alert"
          data-testid="admin-recovery-notice"
          className={cn(
            "flex items-start gap-2 rounded-md border p-3 text-sm",
            mfTone.warning.panel,
            mfTone.warning.text,
          )}
        >
          <ShieldAlert className="mt-0.5 h-4 w-4 shrink-0" aria-hidden />
          <div className="flex-1 space-y-1">
            <p className="font-semibold">{t("session.recoveryNotice.title")}</p>
            <p>
              {t("session.recoveryNotice.message", {
                date: formatDateTime(notice.created_at, i18n.language || "fr"),
                name: notice.created_display_name,
                username: notice.created_username,
                device: notice.redeemed_device_code,
              })}
            </p>
          </div>
          <Button
            type="button"
            size="sm"
            variant="outline"
            disabled={busyId === notice.id}
            onClick={() => void acknowledge(notice.id)}
          >
            {t("session.recoveryNotice.acknowledge")}
          </Button>
        </div>
      ))}
      {error ? (
        <p role="alert" className="text-sm text-text-danger">
          {error}
        </p>
      ) : null}
    </div>
  );
}
