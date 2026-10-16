import { useTranslation } from "react-i18next";

import { windowScopeLabel } from "@/components/planning/ProductionWindowDialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { formatOrDash } from "@/lib/display";
import type { PlanningWindow } from "@shared/ipc-types";

type PlanningWindowDetailDialogProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  window: PlanningWindow | null;
};

export function PlanningWindowDetailDialog({
  open,
  onOpenChange,
  window: planningWindow,
}: PlanningWindowDetailDialogProps) {
  const { t, i18n } = useTranslation("planning");

  const typeLabel = planningWindow
    ? t(`windows.types.${planningWindow.window_type}`, {
        defaultValue: planningWindow.window_type,
      })
    : t("windows.detailTitle");

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle className="flex flex-wrap items-center gap-2">
            <span>{t("windows.detailTitle")}</span>
            {planningWindow ? <Badge variant="secondary">{typeLabel}</Badge> : null}
            {planningWindow?.is_locked ? (
              <Badge variant="outline">{t("windows.locked")}</Badge>
            ) : null}
          </DialogTitle>
        </DialogHeader>

        {planningWindow ? (
          <div className="grid gap-3 text-sm">
            <div>
              <div className="text-[10px] uppercase tracking-wide text-text-muted">
                {t("windows.type")}
              </div>
              <div className="text-text-primary">{typeLabel}</div>
            </div>
            <div>
              <div className="text-[10px] uppercase tracking-wide text-text-muted">
                {t("windows.site")}
              </div>
              <div className="text-text-primary">
                {windowScopeLabel(planningWindow, t("windows.plantWide"))}
              </div>
              {planningWindow.scope_review_required === 1 ? (
                <p className="mt-1 text-xs text-amber-800">{t("windows.scopeReviewHint")}</p>
              ) : null}
            </div>
            <div>
              <div className="text-[10px] uppercase tracking-wide text-text-muted">
                {t("windows.start")}
              </div>
              <div className="text-text-primary">
                {new Date(planningWindow.start_datetime).toLocaleString(i18n.language)}
              </div>
            </div>
            <div>
              <div className="text-[10px] uppercase tracking-wide text-text-muted">
                {t("windows.end")}
              </div>
              <div className="text-text-primary">
                {new Date(planningWindow.end_datetime).toLocaleString(i18n.language)}
              </div>
            </div>
            <div>
              <div className="text-[10px] uppercase tracking-wide text-text-muted">
                {t("windows.reason")}
              </div>
              <div className="text-text-primary">{formatOrDash(planningWindow.lock_reason)}</div>
            </div>
          </div>
        ) : (
          <p className="text-sm text-text-muted">{t("windows.detailEmpty")}</p>
        )}

        <DialogFooter>
          <Button type="button" size="sm" variant="outline" onClick={() => onOpenChange(false)}>
            {t("dialogs.close")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
