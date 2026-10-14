import { Settings2 } from "lucide-react";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
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
import { StatusChip } from "@/components/ui/status-chip";
import { Switch } from "@/components/ui/switch";
import { useFormatters } from "@/hooks/use-formatters";
import { usePmStore } from "@/stores/pm-store";
import { P } from "@shared/rbac/permissions.generated";

import { pmUnreportedErrorText } from "./pm-errors";

/** Six-field cron (second minute hour day month weekday) of the two schedule presets. */
const CRON_DAILY_MIDNIGHT = "0 0 0 * * *";
const CRON_DAILY_06 = "0 0 6 * * *";
/** Starting horizon before the control row is read (the form opens from the stored value). */
const DEFAULT_HORIZON_DAYS = 30;
const MAX_HORIZON_DAYS = 365;

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
  const [horizonDays, setHorizonDays] = useState(DEFAULT_HORIZON_DAYS);
  const [error, setError] = useState<string | null>(null);

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
    setError(null);
    setOpen(true);
  };

  const saveSettings = async () => {
    const cron_expr =
      preset === "midnight"
        ? CRON_DAILY_MIDNIGHT
        : preset === "morning"
          ? CRON_DAILY_06
          : customCron.trim();
    setError(null);
    try {
      await updateGenerationSettings({
        enabled,
        cron_expr,
        horizon_days: horizonDays,
      });
      setOpen(false);
    } catch (err) {
      // Reported by the global toast when it came from the backend; the popover stays open.
      setError(pmUnreportedErrorText(err));
    }
  };

  return (
    <div className="mx-4 mt-3 flex flex-wrap items-center gap-3 rounded-lg border border-border bg-card px-3 py-2 text-sm">
      <StatusChip
        label={control?.enabled ? t("autopilot.autoOn") : t("autopilot.autoOff")}
        tone={control?.enabled ? "success" : "neutral"}
        dot
      />
      <span className="text-text-muted">
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
              <Label htmlFor="pm-autopilot-schedule">{t("autopilot.schedule")}</Label>
              <Select
                value={preset}
                onValueChange={(v) => setPreset(v as "midnight" | "morning" | "custom")}
              >
                <SelectTrigger id="pm-autopilot-schedule">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="midnight">{t("autopilot.cronMidnight")}</SelectItem>
                  <SelectItem value="morning">{t("autopilot.cronMorning")}</SelectItem>
                  <SelectItem value="custom">{t("autopilot.cronCustom")}</SelectItem>
                </SelectContent>
              </Select>
              {preset === "custom" ? (
                <>
                  <Input
                    aria-label={t("autopilot.cronCustom")}
                    value={customCron}
                    onChange={(e) => setCustomCron(e.target.value)}
                  />
                  <p className="text-xs text-text-muted">{t("autopilot.cronCustomHint")}</p>
                </>
              ) : null}
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="pm-autopilot-horizon">{t("autopilot.horizonDays")}</Label>
              <Input
                id="pm-autopilot-horizon"
                type="number"
                min={1}
                max={MAX_HORIZON_DAYS}
                value={horizonDays}
                onChange={(e) =>
                  setHorizonDays(
                    Math.min(MAX_HORIZON_DAYS, Math.max(1, Number(e.target.value) || 1)),
                  )
                }
              />
            </div>
            {error ? (
              <p role="alert" className="text-sm text-tone-danger-fg">
                {error}
              </p>
            ) : null}
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
