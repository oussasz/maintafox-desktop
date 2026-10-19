// Standing warning while the signed-in account is the only active administrator of the plant
// (`auth::last_admin`). It cannot be dismissed; it goes away once a second administrator exists.

import { ShieldAlert } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { mfTone } from "@/design-system/tone";
import { cn } from "@/lib/utils";
import { getSingleAdministratorWarning } from "@/services/single-admin-service";

/** Re-read while signed in, so the warning goes soon after a second administrator is created. */
const REFRESH_INTERVAL_MS = 60 * 1000;

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

  if (!sole) return null;

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
      <div className="space-y-1">
        <p className="font-semibold">{t("session.singleAdministrator.title")}</p>
        <p>{t("session.singleAdministrator.message")}</p>
      </div>
    </div>
  );
}
