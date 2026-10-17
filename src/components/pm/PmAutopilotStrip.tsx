import { Settings2 } from "lucide-react";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { StatusChip } from "@/components/ui/status-chip";
import { Switch } from "@/components/ui/switch";
import { useFormatters } from "@/hooks/use-formatters";
import { usePmStore } from "@/stores/pm-store";
import { P } from "@shared/rbac/permissions.generated";

import { pmUnreportedErrorText } from "./pm-errors";

/** Starting horizon before the control row is read (the form opens from the stored value). */
const DEFAULT_HORIZON_DAYS = 30;
const MAX_HORIZON_DAYS = 365;

/**
 * Daily time of a six-field cron "second minute hour * * *" (the only shape this screen
 * edits), as "HH:MM"; null for any other schedule (kept as is and shown as "custom").
 */
export function dailyTimeOfCron(cron: string): string | null {
  const parts = cron.trim().split(/\s+/);
  if (parts.length !== 6) return null;
  const [sec, min, hour, dom, mon, dow] = parts;
  if (sec !== "0" || dom !== "*" || mon !== "*" || dow !== "*") return null;
  const m = Number(min);
  const h = Number(hour);
  if (!Number.isInteger(m) || !Number.isInteger(h) || m < 0 || m > 59 || h < 0 || h > 23) {
    return null;
  }
  return `${String(h).padStart(2, "0")}:${String(m).padStart(2, "0")}`;
}

/** "HH:MM" → six-field cron running every day at that time. */
export function cronOfDailyTime(time: string): string {
  const [h, m] = time.split(":").map((x) => Number(x));
  return `0 ${m ?? 0} ${h ?? 0} * * *`;
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
  /** Daily run time being edited; null keeps a schedule this screen cannot express. */
  const [dailyTime, setDailyTime] = useState<string | null>("00:00");
  const [horizonDays, setHorizonDays] = useState(DEFAULT_HORIZON_DAYS);
  const [error, setError] = useState<string | null>(null);

  const lastRunLabel = useMemo(() => {
    if (!control?.last_run_at) return t("autopilot.neverRun");
    return formatDateTime(control.last_run_at);
  }, [control?.last_run_at, formatDateTime, t]);

  const openSettings = () => {
    if (!control) return;
    setEnabled(control.enabled);
    setDailyTime(dailyTimeOfCron(control.cron_expr));
    setHorizonDays(control.horizon_days);
    setError(null);
    setOpen(true);
  };

  const saveSettings = async () => {
    if (!control) return;
    // A custom schedule (not daily) is kept unchanged unless the user switches to a daily time.
    const cron_expr = dailyTime == null ? control.cron_expr : cronOfDailyTime(dailyTime);
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

  const storedDaily = control ? dailyTimeOfCron(control.cron_expr) : null;
  const scheduleText = !control
    ? null
    : storedDaily
      ? t("autopilot.everyDayAt", { time: storedDaily })
      : t("autopilot.customSchedule");
  const known = (n: number | null | undefined) => (n == null ? "—" : String(n));

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
          <span className="ms-2 tabular-nums">
            (
            {t("autopilot.counts", {
              generated: known(control.last_generated_count),
              skipped: known(control.last_skipped_count),
            })}
            )
          </span>
        ) : null}
      </span>
      {scheduleText ? <span className="text-text-muted">{scheduleText}</span> : null}
      <PermissionGate permission={P.PM_EDIT}>
        <Popover open={open} onOpenChange={setOpen}>
          <PopoverTrigger asChild>
            <Button variant="outline" className="h-10" onClick={openSettings} disabled={!control}>
              <Settings2 className="me-1 h-4 w-4" aria-hidden />
              {t("autopilot.settings")}
            </Button>
          </PopoverTrigger>
          <PopoverContent className="w-80 space-y-3" align="start">
            <div className="flex items-center justify-between gap-2">
              <Label htmlFor="pm-autopilot-enabled">{t("autopilot.enabled")}</Label>
              <Switch id="pm-autopilot-enabled" checked={enabled} onCheckedChange={setEnabled} />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="pm-autopilot-time">{t("autopilot.runTime")}</Label>
              {dailyTime == null && control ? (
                <div className="space-y-2 rounded-md border border-surface-border p-2 text-xs">
                  <p className="text-text-secondary">{t("autopilot.customScheduleKept")}</p>
                  <details>
                    <summary className="cursor-pointer select-none text-text-muted">
                      {t("autopilot.technicalSchedule")}
                    </summary>
                    <p className="mt-1 font-mono">{control.cron_expr}</p>
                  </details>
                  <Button variant="outline" className="h-9" onClick={() => setDailyTime("06:00")}>
                    {t("autopilot.switchToDaily")}
                  </Button>
                </div>
              ) : (
                <>
                  <Input
                    id="pm-autopilot-time"
                    type="time"
                    className="h-10"
                    value={dailyTime ?? "00:00"}
                    onChange={(e) => setDailyTime(e.target.value || "00:00")}
                  />
                  <p className="text-xs text-text-muted">{t("autopilot.runTimeHint")}</p>
                </>
              )}
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
            <Button className="h-10 w-full" disabled={saving} onClick={() => void saveSettings()}>
              {t("actions.save")}
            </Button>
          </PopoverContent>
        </Popover>
      </PermissionGate>
    </div>
  );
}
