// Persistent notice to each settings administrator that a safety rule (a site's four-eyes
// opt-out, a permit type's three-person lock removal) was changed on another PC and applied here
// (`settings::received_notice`). Shown after every sign-in on this PC until that administrator
// acknowledges it (CMMS Advisor, as the recovery-code notice); the acknowledgement is audited by the
// backend. It cannot be archived, snoozed or deleted before.

import { ShieldAlert } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { notificationText } from "@/components/notifications/notification-text";
import { Button } from "@/components/ui/button";
import { mfTone } from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import { appErrorText } from "@/lib/app-error";
import { cn } from "@/lib/utils";
import {
  type SafetyRuleNotice as Notice,
  acknowledgeSafetyRuleNotice,
  getSafetyRuleNotices,
} from "@/services/notification-service";

/** The category of the notice (`settings::received_notice::RECEIVED_RULE_CATEGORY`). */
export const SAFETY_NOTICE_CATEGORY = "safety_rule_received";
/**
 * Every category of a persistent safety notice (`settings::received_notice::SAFETY_NOTICE_CATEGORIES`):
 * a safety rule changed on another PC, a lock taken off by a forced removal (shown to the
 * permit close-right holders of the site), and two concurrent versions of a PM plan (shown to the
 * PM planners). Each recipient acknowledges their own.
 */
export const SAFETY_NOTICE_CATEGORIES: ReadonlySet<string> = new Set([
  SAFETY_NOTICE_CATEGORY,
  "loto_forced_removal",
  "pm_concurrent_versions",
]);
/** Sent when a notice is acknowledged elsewhere (the inbox): the banner reads its list again. */
export const SAFETY_NOTICE_ACKNOWLEDGED_EVENT = "maintafox:safety-notice-acknowledged";

interface SafetyRuleNoticeProps {
  /** Re-read when the signed-in account changes (its user id, or null when signed out). */
  sessionUserId: number | null;
  className?: string;
}

export function SafetyRuleNotice({ sessionUserId, className }: SafetyRuleNoticeProps) {
  const { t } = useTranslation("notifications");
  const fmt = useFormatters();
  const [notices, setNotices] = useState<Notice[]>([]);
  const [busyId, setBusyId] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loadFailed, setLoadFailed] = useState(false);

  const load = useCallback(async (isCancelled: () => boolean) => {
    try {
      const next = await getSafetyRuleNotices();
      if (!isCancelled()) {
        setNotices(next);
        setLoadFailed(false);
      }
    } catch {
      // The notices stay stored; say they could not be read rather than show nothing.
      if (!isCancelled()) {
        setNotices([]);
        setLoadFailed(true);
      }
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
    const reload = () => void load(() => cancelled);
    window.addEventListener(SAFETY_NOTICE_ACKNOWLEDGED_EVENT, reload);
    return () => {
      cancelled = true;
      window.removeEventListener(SAFETY_NOTICE_ACKNOWLEDGED_EVENT, reload);
    };
  }, [sessionUserId, load]);

  const acknowledge = async (id: number) => {
    setBusyId(id);
    setError(null);
    try {
      await acknowledgeSafetyRuleNotice(id);
      setNotices((current) => current.filter((n) => n.id !== id));
    } catch (err) {
      setError(appErrorText(err));
    } finally {
      setBusyId(null);
    }
  };

  if (notices.length === 0 && !loadFailed) return null;

  return (
    <div role="alert" className={cn("space-y-2", className)} data-testid="safety-rule-notices">
      {loadFailed ? (
        <p className="text-sm text-text-danger" data-testid="safety-rule-notices-error">
          {t("safetyNotice.loadError")}
        </p>
      ) : null}
      {notices.map((notice) => {
        const text = notificationText(notice, t, fmt);
        const titleId = `safety-rule-notice-${notice.id}`;
        return (
          <div
            key={notice.id}
            data-testid="safety-rule-notice"
            className={cn(
              "flex items-start gap-2 rounded-md border p-3 text-sm",
              mfTone.warning.panel,
              mfTone.warning.text,
            )}
          >
            <ShieldAlert className="mt-0.5 h-4 w-4 shrink-0" aria-hidden />
            <div className="flex-1 space-y-1">
              <p id={titleId} className="font-semibold">
                {text.title}
              </p>
              {text.body ? <p>{text.body}</p> : null}
              <p className="text-xs">{t("safetyNotice.hint")}</p>
            </div>
            <Button
              type="button"
              size="sm"
              variant="outline"
              disabled={busyId === notice.id}
              aria-describedby={titleId}
              onClick={() => void acknowledge(notice.id)}
            >
              {t("safetyNotice.acknowledge")}
            </Button>
          </div>
        );
      })}
      {error ? <p className="text-sm text-text-danger">{error}</p> : null}
    </div>
  );
}
