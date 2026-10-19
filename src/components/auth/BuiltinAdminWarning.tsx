// Warning shown to the built-in "admin" account whenever it is signed in (`auth::builtin_admin`).
// It cannot be dismissed. While that account is the only administrator it asks for a named
// administrator; once one exists it asks to disable this account (nothing disables it
// automatically once its password has been chosen).

import { AlertTriangle } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { mfTone } from "@/design-system/tone";
import { cn } from "@/lib/utils";
import {
  type BuiltinAdminWarning as BuiltinAdminWarningState,
  builtinAdminWarningMessage,
  builtinAdminWarningVisible,
  getBuiltinAdminWarning,
} from "@/services/builtin-admin-service";

/** Re-read while shown, so its text follows soon after a named administrator is created. */
const REFRESH_INTERVAL_MS = 60 * 1000;

interface BuiltinAdminWarningProps {
  /** Re-read when the signed-in account changes (its user id, or null when signed out). */
  sessionUserId: number | null;
  className?: string;
}

export function BuiltinAdminWarning({ sessionUserId, className }: BuiltinAdminWarningProps) {
  const { t } = useTranslation("auth");
  const [warning, setWarning] = useState<BuiltinAdminWarningState | null>(null);

  useEffect(() => {
    if (sessionUserId === null) {
      setWarning(null);
      return;
    }
    let cancelled = false;
    const load = () => {
      getBuiltinAdminWarning()
        .then((next) => {
          if (!cancelled) setWarning(next);
        })
        .catch(() => {
          // No answer: nothing can be stated; the account rules still apply backend-side.
          if (!cancelled) setWarning(null);
        });
    };
    load();
    const id = window.setInterval(load, REFRESH_INTERVAL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, [sessionUserId]);

  if (warning === null || !builtinAdminWarningVisible(warning)) return null;

  return (
    <div
      role="alert"
      data-testid="builtin-admin-warning"
      className={cn(
        "flex items-start gap-2 rounded-md border p-3 text-sm",
        mfTone.warning.panel,
        mfTone.warning.text,
        className,
      )}
    >
      <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" aria-hidden />
      <div className="space-y-1">
        <p className="font-semibold">{t("session.builtinAdmin.title")}</p>
        <p>{t(`session.builtinAdmin.${builtinAdminWarningMessage(warning)}`)}</p>
      </div>
    </div>
  );
}
