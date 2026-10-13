import { Settings2 } from "lucide-react";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { useFormatters } from "@/hooks/use-formatters";
import { usePmStore } from "@/stores/pm-store";
import { P } from "@shared/rbac/permissions.generated";

const CRON_DAILY_MIDNIGHT = "0 0 0 * * *";
const CRON_DAILY_06 = "0 0 6 * * *";

function cronPreset(cron: string): "midnight" | "morning" | "custom" {
  if (cron.trim() === CRON_DAILY_MIDNIGHT) return "midnight";
  if (cron.trim() === CRON_DAILY_06) return "morning";
  return "custom";
}

export function PmAutopilotStrip() {
  const { t } = useTranslation("pm");
  const { formatDateTime } = useFormatters();
  const generationStatus = usePmStore((s) => s.generationStatus);
  const saving = usePmStore((s) => s.saving);
  const updateGenerationSettings = usePmStore((s) => s.updateGenerationSettings);

  const control = generationStatus?.control;
  const [open, setOpen] = useState(false);
  const [enabled, setEnabled] = useState(true);
  const [preset, setPreset] = useState<"midnight" | "morning" | "custom">("midnight");
  const [customCron, setCustomCron] = useState(CRON_DAILY_MIDNIGHT);
  const [horizonDays, setHorizonDays] = useState(30);

  const lastRunLabel = useMemo(() => {
    if (!control?.last_run_at) return t("autopilot.neverRun");
    return formatDateTime(control.last_run_at);
  }, [control?.last_run_at, formatDateTime, t]);

  const openSettings = () => {
    if (!control) return;
    setEnabled(control.enabled);
    const p = cronPreset(control.cron_expr);
    setPreset(p);
    setCustomCron(control.cron_expr);
    setHorizonDays(control.horizon_days);
    setOpen(true);
  };

  const saveSettings = async () => {
    const cron_expr =
      preset === "midnight"
        ? CRON_DAILY_MIDNIGHT
        : preset === "morning"
          ? CRON_DAILY_06
          : customCron.trim();
    await updateGenerationSettings({
      enabled,
      cron_expr,
      horizon_days: horizonDays,
    });
    setOpen(false);
  };

  return (
    <div className="mx-4 mt-3 flex flex-wrap items-center gap-3 rounded-lg border border-border bg-card px-3 py-2 text-sm">
      <Badge variant={control?.enabled ? "default" : "secondary"}>
        {control?.enabled ? t("autopilot.autoOn") : t("autopilot.autoOff")}
      </Badge>
      <span className="text-muted-foreground">
        {t("autopilot.lastRun")}: <span className="text-foreground">{lastRunLabel}</span>
        {control?.last_generated_count != null || control?.last_skipped_count != null ? (
          <span className="ml-2 tabular-nums">
            (
            {t("autopilot.counts", {
              generated: control.last_generated_count ?? 0,
              skipped: control.last_skipped_count ?? 0,
            })}
            )
          </span>
        ) : null}
      </span>
      <PermissionGate permission={P.PM_EDIT}>
        <Popover open={open} onOpenChange={setOpen}>
          <PopoverTrigger asChild>
            <Button size="sm" variant="outline" onClick={openSettings} disabled={!control}>
              <Settings2 className="mr-1 h-3.5 w-3.5" />
              {t("autopilot.settings")}
            </Button>
          </PopoverTrigger>
          <PopoverContent className="w-80 space-y-3" align="start">
            <div className="flex items-center justify-between gap-2">
              <Label htmlFor="pm-autopilot-enabled">{t("autopilot.enabled")}</Label>
              <Switch id="pm-autopilot-enabled" checked={enabled} onCheckedChange={setEnabled} />
            </div>
            <div className="space-y-1.5">
              <Label>{t("autopilot.schedule")}</Label>
              <Select
                value={preset}
                onValueChange={(v) => setPreset(v as "midnight" | "morning" | "custom")}
              >
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="midnight">{t("autopilot.cronMidnight")}</SelectItem>
                  <SelectItem value="morning">{t("autopilot.cronMorning")}</SelectItem>
                  <SelectItem value="custom">{t("autopilot.cronCustom")}</SelectItem>
                </SelectContent>
              </Select>
              {preset === "custom" ? (
                <Input
                  value={customCron}
                  onChange={(e) => setCustomCron(e.target.value)}
                  placeholder="0 0 0 * * *"
                />
              ) : null}
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="pm-autopilot-horizon">{t("autopilot.horizonDays")}</Label>
              <Input
                id="pm-autopilot-horizon"
                type="number"
                min={1}
                max={365}
                value={horizonDays}
                onChange={(e) => setHorizonDays(Number(e.target.value) || 30)}
              />
            </div>
            <Button
              size="sm"
              className="w-full"
              disabled={saving}
              onClick={() => void saveSettings()}
            >
              {t("actions.save")}
            </Button>
          </PopoverContent>
        </Popover>
      </PermissionGate>
    </div>
  );
}
