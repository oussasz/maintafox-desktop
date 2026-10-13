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
import type { GeneratePmOccurrencesInput, PmPlan } from "@shared/ipc-types";

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
    setHorizonDays(30);
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
      setError(err instanceof Error ? err.message : String(err));
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
            <Label>{t("fields.horizonDays")}</Label>
            <Input
              type="number"
              min={1}
              max={365}
              value={horizonDays}
              onChange={(e) => setHorizonDays(Number(e.target.value) || 30)}
            />
          </div>
          <div className="grid gap-1">
            <Label>{t("fields.forecastScope")}</Label>
            <Select value={scope} onValueChange={(v) => setScope(v as "all" | "selected")}>
              <SelectTrigger>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="all">{t("fields.forecastAllPlans")}</SelectItem>
                <SelectItem value="selected" disabled={activePlanId == null}>
                  {t("fields.forecastSelectedPlan")}
                </SelectItem>
              </SelectContent>
            </Select>
            <p className="text-xs text-muted-foreground">{eligible.length} eligible plan(s)</p>
          </div>
          <div className="grid gap-1">
            <Label>{t("fields.eventCodesOptional")}</Label>
            <Input
              value={eventCodes}
              onChange={(e) => setEventCodes(e.target.value)}
              placeholder="CODE_A, CODE_B"
            />
          </div>
          <div className="grid gap-1">
            <Label>{t("fields.conditionCodesOptional")}</Label>
            <Input
              value={conditionCodes}
              onChange={(e) => setConditionCodes(e.target.value)}
              placeholder="CODE_A, CODE_B"
            />
          </div>
          {error ? <p className="text-sm text-destructive">{error}</p> : null}
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
