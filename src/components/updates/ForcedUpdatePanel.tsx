import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { useUpdateScheduler, useUpdater } from "@/hooks/use-updater";
import { useAppStore } from "@/store/app-store";

import { UpdateBanner } from "./UpdateBanner";

/**
 * The update path while the licence requires an update (`denied_force_update_required`): the
 * licence screen replaces the whole app then, so the update the licence asks for is checked,
 * offered and installed from here (the same banner and install flow as in the app). The licence
 * stays denied until the installed version's licence check lifts it (`product_license`): nothing
 * here unblocks the app. When no update is published for this PC yet, it says so and stops (a
 * check runs at most when asked, or on the usual schedule).
 */
export function ForcedUpdatePanel() {
  const { t } = useTranslation("shell");
  const ready = useAppStore((s) => s.appStatus === "ready");
  useUpdateScheduler(ready);
  const u = useUpdater();
  const status = u.result?.status;
  const statusMessage =
    status === "not_configured" || status === "not_activated" || status === "unreachable"
      ? t(`updates.status.${status}`)
      : null;

  return (
    <div className="mt-6 space-y-3" data-testid="forced-update-panel">
      <UpdateBanner embedded />
      {statusMessage ? <p className="text-sm text-text-secondary">{statusMessage}</p> : null}
      <p className="text-sm text-text-secondary">{t("updates.forced.adminHint")}</p>
      {u.phase === "idle" ? (
        <Button size="sm" variant="outline" onClick={u.checkNow} disabled={u.isChecking || !ready}>
          {u.isChecking ? t("updates.status.checking") : t("updates.forced.check")}
        </Button>
      ) : null}
    </div>
  );
}
