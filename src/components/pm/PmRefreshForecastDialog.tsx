import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { PmPlanRow as PmPlan } from "@/services/pm-service";
import type { GeneratePmOccurrencesInput } from "@shared/ipc-types";

import { pmUnreportedErrorText } from "./pm-errors";

/** Forecast horizon offered by default and its bounds (backend clamps to 1–365). */
const DEFAULT_HORIZON_DAYS = 30;
const MAX_HORIZON_DAYS = 365;

interface PmRefreshForecastDialogProps {
  open: boolean;
  plans: PmPlan[];
  activePlanId: number | null;
  saving?: boolean;
  onClose: () => void;
  onRun: (input: GeneratePmOccurrencesInput) => Promise<void>;
}

export function PmRefreshForecastDialog({
  open,
  plans,
  activePlanId,
  saving,
  onClose,
  onRun,
}: PmRefreshForecastDialogProps) {
  const { t } = useTranslation("pm");
  const [horizonDays, setHorizonDays] = useState(30);
  const [scope, setScope] = useState<"all" | "selected">("all");
  const [eventCodes, setEventCodes] = useState("");
  const [conditionCodes, setConditionCodes] = useState("");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    setHorizonDays(DEFAULT_HORIZON_DAYS);
    setScope(activePlanId != null ? "selected" : "all");
    setEventCodes("");
    setConditionCodes("");
    setError(null);
  }, [open, activePlanId]);

  const eligible = plans.filter(
    (p) =>
      p.is_active === 1 &&
      (p.lifecycle_status === "active" || p.lifecycle_status === "suspended") &&
      p.current_version_id != null,
  );

  async function handleRun() {
    setError(null);
    if (scope === "selected" && activePlanId == null) {
      setError(t("hints.generationNeedLifecycle"));
      return;
    }
    const parseCodes = (raw: string) =>
      raw
        .split(/[,;\s]+/)
        .map((c) => c.trim())
        .filter(Boolean);

    try {
      await onRun({
        pm_plan_id: scope === "selected" ? activePlanId : null,
        horizon_days: horizonDays,
        as_of: null,
        event_codes: parseCodes(eventCodes),
        condition_codes: parseCodes(conditionCodes),
      });
      onClose();
    } catch (err) {
      setError(pmUnreportedErrorText(err));
    }
  }

  return (
    <Dialog open={open} onOpenChange={(v) => !v && onClose()}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>{t("dialogs.refreshForecast.title")}</DialogTitle>
          <DialogDescription>{t("dialogs.refreshForecast.description")}</DialogDescription>
        </DialogHeader>
        <div className="grid gap-3 py-2">
          <div className="grid gap-1">
            <Label htmlFor="pm-forecast-horizon">{t("fields.horizonDays")}</Label>
            <Input
              id="pm-forecast-horizon"
              type="number"
              min={1}
              max={MAX_HORIZON_DAYS}
              value={horizonDays}
              onChange={(e) =>
                setHorizonDays(Math.min(MAX_HORIZON_DAYS, Math.max(1, Number(e.target.value) || 1)))
              }
            />
          </div>
          <div className="grid gap-1">
            <Label htmlFor="pm-forecast-scope">{t("fields.forecastScope")}</Label>
            <Select value={scope} onValueChange={(v) => setScope(v as "all" | "selected")}>
              <SelectTrigger id="pm-forecast-scope">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="all">{t("fields.forecastAllPlans")}</SelectItem>
                <SelectItem value="selected" disabled={activePlanId == null}>
                  {t("fields.forecastSelectedPlan")}
                </SelectItem>
              </SelectContent>
            </Select>
            <p className="text-xs text-text-muted">
              {t("dialogs.refreshForecast.eligiblePlans", { count: eligible.length })}
            </p>
          </div>
          <div className="grid gap-1">
            <Label htmlFor="pm-forecast-events">{t("fields.eventCodesOptional")}</Label>
            <Input
              id="pm-forecast-events"
              value={eventCodes}
              onChange={(e) => setEventCodes(e.target.value)}
              placeholder={t("fields.codesPlaceholder")}
            />
          </div>
          <div className="grid gap-1">
            <Label htmlFor="pm-forecast-conditions">{t("fields.conditionCodesOptional")}</Label>
            <Input
              id="pm-forecast-conditions"
              value={conditionCodes}
              onChange={(e) => setConditionCodes(e.target.value)}
              placeholder={t("fields.codesPlaceholder")}
            />
          </div>
          {error ? (
            <p role="alert" className="text-sm text-tone-danger-fg">
              {error}
            </p>
          ) : null}
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            {t("actions.cancel")}
          </Button>
          <Button disabled={saving} onClick={() => void handleRun()}>
            {t("actions.runForecast")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
