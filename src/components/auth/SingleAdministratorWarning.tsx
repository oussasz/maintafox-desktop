// Warning while the signed-in account is the only active administrator of the plant
// (`auth::last_admin`). It goes away once a second administrator exists. The user can hide it
// for the current session (it comes back at the next launch), so it does not push every page down.

import { ShieldAlert, X } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { mfTone } from "@/design-system/tone";
import { cn } from "@/lib/utils";
import { getSingleAdministratorWarning } from "@/services/single-admin-service";

/** Re-read while signed in, so the warning goes soon after a second administrator is created. */
const REFRESH_INTERVAL_MS = 60 * 1000;

/** Users who hid the warning in this session; kept at module level so it survives page changes. */
const hiddenForSession = new Set<number>();

interface SingleAdministratorWarningProps {
  /** Re-read when the signed-in account changes (its user id, or null when signed out). */
  sessionUserId: number | null;
  className?: string;
}

export function SingleAdministratorWarning({
  sessionUserId,
  className,
}: SingleAdministratorWarningProps) {
  const { t } = useTranslation("auth");
  const [sole, setSole] = useState(false);
  const [, setHiddenTick] = useState(0);

  useEffect(() => {
    if (sessionUserId === null) {
      setSole(false);
      return;
    }
    let cancelled = false;
    const load = () => {
      getSingleAdministratorWarning()
        .then((next) => {
          if (!cancelled) setSole(next.sole_active_administrator);
        })
        .catch(() => {
          // No answer: nothing can be stated; the backend still refuses to remove the last one.
          if (!cancelled) setSole(false);
        });
    };
    load();
    const id = window.setInterval(load, REFRESH_INTERVAL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, [sessionUserId]);

  if (!sole || (sessionUserId !== null && hiddenForSession.has(sessionUserId))) return null;

  return (
    <div
      role="alert"
      data-testid="single-administrator-warning"
      className={cn(
        "flex items-start gap-2 rounded-md border p-3 text-sm",
        mfTone.warning.panel,
        mfTone.warning.text,
        className,
      )}
    >
      <ShieldAlert className="mt-0.5 h-4 w-4 shrink-0" aria-hidden />
      <div className="min-w-0 flex-1 space-y-1">
        <p className="font-semibold">{t("session.singleAdministrator.title")}</p>
        <p>{t("session.singleAdministrator.message")}</p>
      </div>
      <Button
        type="button"
        variant="ghost"
        size="sm"
        className="shrink-0 gap-1"
        onClick={() => {
          if (sessionUserId !== null) hiddenForSession.add(sessionUserId);
          setHiddenTick((n) => n + 1);
        }}
      >
        <X className="h-4 w-4" aria-hidden />
        {t("session.singleAdministrator.dismiss")}
      </Button>
    </div>
  );
}
