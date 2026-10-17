import { CalendarClock, Plus, Trash2 } from "lucide-react";
import { type ReactNode, useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  type BuildCronResult,
  DEFAULT_SCHEDULE_SPEC,
  MAX_MONTH_DAY,
  type ScheduleFrequency,
  type ScheduleSpec,
  buildScheduleCron,
  localUtcOffsetMinutes,
  parseScheduleCron,
} from "@/components/analytics/report-schedule-cron";
import {
  reportFormatLabel,
  reportTemplateTitle,
} from "@/components/analytics/report-template-label";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { EmptyState } from "@/components/ui/empty-state";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { TimeInput } from "@/components/ui/time-input";
import { mfTable } from "@/design-system/tokens";
import { useFormatters } from "@/hooks/use-formatters";
import { usePermissions } from "@/hooks/use-permissions";
import { cn } from "@/lib/utils";
import {
  deleteMyReportSchedule,
  listMyReportSchedules,
  listReportTemplates,
  upsertMyReportSchedule,
} from "@/services/reports-service";
import type { ReportSchedule, ReportTemplate } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

type ExportFormat = "pdf" | "xlsx";

const WEEKDAYS = [1, 2, 3, 4, 5, 6, 0] as const; // Monday first
const MONTH_DAYS = Array.from({ length: MAX_MONTH_DAY }, (_, i) => i + 1);

function useScheduleText() {
  const { t } = useTranslation("reports");
  const { locale, formatTime } = useFormatters();
  const weekdayName = useCallback(
    (day: number) =>
      new Intl.DateTimeFormat(locale === "en" ? "en-US" : "fr-FR", {
        weekday: "long",
        timeZone: "UTC",
      }).format(new Date(Date.UTC(2024, 0, 7 + day))), // 2024-01-07 is a Sunday
    [locale],
  );
  const timeText = useCallback(
    (hhmm: string) => {
      const [h, m] = hhmm.split(":").map(Number);
      return formatTime(new Date(2000, 0, 1, h ?? 0, m ?? 0));
    },
    [formatTime],
  );
  const describe = useCallback(
    (spec: ScheduleSpec) => {
      const time = timeText(spec.time);
      if (spec.frequency === "daily") return t("schedule.describe.daily", { time });
      if (spec.frequency === "weekly") {
        return t("schedule.describe.weekly", { day: weekdayName(spec.weekday), time });
      }
      return t("schedule.describe.monthly", { day: spec.monthDay, time });
    },
    [t, timeText, weekdayName],
  );
  return { weekdayName, describe };
}

export function AnalyticsSchedulesPage() {
  const { t } = useTranslation("reports");
  const { formatDateTime } = useFormatters();
  const { can } = usePermissions();
  const canManage = can(P.REP_MANAGE);
  const { weekdayName, describe } = useScheduleText();

  const [templates, setTemplates] = useState<ReportTemplate[]>([]);
  const [schedules, setSchedules] = useState<ReportSchedule[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<unknown>(null);

  const [templateId, setTemplateId] = useState<number | null>(null);
  const [spec, setSpec] = useState<ScheduleSpec>(DEFAULT_SCHEDULE_SPEC);
  const [exportFormat, setExportFormat] = useState<ExportFormat>("pdf");
  const [enabled, setEnabled] = useState(true);
  const [saving, setSaving] = useState(false);
  const [togglingId, setTogglingId] = useState<number | null>(null);
  const [toDelete, setToDelete] = useState<ReportSchedule | null>(null);

  const offset = localUtcOffsetMinutes();
  const built: BuildCronResult = useMemo(() => buildScheduleCron(spec, offset), [spec, offset]);
  const templateById = useMemo(() => new Map(templates.map((x) => [x.id, x])), [templates]);

  const load = useCallback(() => {
    if (!canManage) return;
    setLoading(true);
    setLoadError(null);
    Promise.all([listReportTemplates(), listMyReportSchedules()])
      .then(([tpl, sched]) => {
        setTemplates(tpl);
        setSchedules(sched);
        setTemplateId((current) =>
          current !== null && tpl.some((x) => x.id === current) ? current : (tpl[0]?.id ?? null),
        );
      })
      .catch((e: unknown) => setLoadError(e))
      .finally(() => setLoading(false));
  }, [canManage]);

  useEffect(() => {
    load();
  }, [load]);

  const onAdd = async () => {
    if (!canManage || templateId === null || !built.ok) return;
    setSaving(true);
    try {
      await upsertMyReportSchedule({
        template_id: templateId,
        cron_expr: built.cron,
        export_format: exportFormat,
        enabled,
      });
      load();
    } catch {
      // The global IPC toast reports the failure.
    } finally {
      setSaving(false);
    }
  };

  const onToggle = async (s: ReportSchedule, next: boolean) => {
    setTogglingId(s.id);
    try {
      await upsertMyReportSchedule({
        id: s.id,
        template_id: s.template_id,
        cron_expr: s.cron_expr,
        export_format: s.export_format,
        enabled: next,
      });
      load();
    } catch {
      // The global IPC toast reports the failure.
    } finally {
      setTogglingId(null);
    }
  };

  if (!canManage) {
    return <p className="text-sm text-text-muted">{t("schedule.noAccess")}</p>;
  }

  const setFrequency = (frequency: ScheduleFrequency) => setSpec((s) => ({ ...s, frequency }));
  const buildError = built.ok ? null : t(`schedule.errors.${built.reason}`);
  const selectedTemplate = templateId !== null ? templateById.get(templateId) : undefined;

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">{t("schedule.title")}</CardTitle>
        <p className="text-xs text-text-muted">{t("schedule.subtitle")}</p>
      </CardHeader>
      <CardContent className="space-y-4">
        {loading && schedules.length === 0 && templates.length === 0 ? (
          <LoadingState label={t("schedule.loading")} />
        ) : loadError ? (
          <ErrorState error={loadError} title={t("schedule.loadError")} onRetry={load} />
        ) : (
          <>
            <fieldset className="space-y-3 rounded-md border border-surface-border p-3">
              <legend className="px-1 text-sm font-medium text-text-primary">
                {t("schedule.newTitle")}
              </legend>
              {templates.length === 0 ? (
                <p className="text-sm text-text-muted">{t("templates.empty")}</p>
              ) : (
                <>
                  <div className="flex flex-wrap items-end gap-3">
                    <Field label={t("schedule.template")} id="sched-template">
                      <Select
                        value={templateId !== null ? String(templateId) : ""}
                        onValueChange={(v) => setTemplateId(Number(v))}
                      >
                        <SelectTrigger id="sched-template" className="h-10 w-64">
                          <SelectValue>
                            {selectedTemplate ? reportTemplateTitle(t, selectedTemplate) : null}
                          </SelectValue>
                        </SelectTrigger>
                        <SelectContent>
                          {templates.map((x) => (
                            <SelectItem key={x.id} value={String(x.id)}>
                              {reportTemplateTitle(t, x)}
                            </SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                    </Field>
                    <Field label={t("schedule.frequency")} id="sched-frequency">
                      <Select
                        value={spec.frequency}
                        onValueChange={(v) => setFrequency(v as ScheduleFrequency)}
                      >
                        <SelectTrigger id="sched-frequency" className="h-10 w-44">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="daily">{t("schedule.frequencies.daily")}</SelectItem>
                          <SelectItem value="weekly">{t("schedule.frequencies.weekly")}</SelectItem>
                          <SelectItem value="monthly">
                            {t("schedule.frequencies.monthly")}
                          </SelectItem>
                        </SelectContent>
                      </Select>
                    </Field>
                    {spec.frequency === "weekly" ? (
                      <Field label={t("schedule.weekday")} id="sched-weekday">
                        <Select
                          value={String(spec.weekday)}
                          onValueChange={(v) => setSpec((s) => ({ ...s, weekday: Number(v) }))}
                        >
                          <SelectTrigger id="sched-weekday" className="h-10 w-40">
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            {WEEKDAYS.map((d) => (
                              <SelectItem key={d} value={String(d)}>
                                {weekdayName(d)}
                              </SelectItem>
                            ))}
                          </SelectContent>
                        </Select>
                      </Field>
                    ) : null}
                    {spec.frequency === "monthly" ? (
                      <Field label={t("schedule.monthDay")} id="sched-monthday">
                        <Select
                          value={String(spec.monthDay)}
                          onValueChange={(v) => setSpec((s) => ({ ...s, monthDay: Number(v) }))}
                        >
                          <SelectTrigger id="sched-monthday" className="h-10 w-28">
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            {MONTH_DAYS.map((d) => (
                              <SelectItem key={d} value={String(d)}>
                                {d}
                              </SelectItem>
                            ))}
                          </SelectContent>
                        </Select>
                      </Field>
                    ) : null}
                    <Field label={t("schedule.time")} id="sched-time">
                      <TimeInput
                        id="sched-time"
                        className="h-10 w-32"
                        value={spec.time}
                        onChange={(time) => setSpec((s) => ({ ...s, time }))}
                      />
                    </Field>
                    <Field label={t("schedule.format")} id="sched-format">
                      <Select
                        value={exportFormat}
                        onValueChange={(v) => setExportFormat(v as ExportFormat)}
                      >
                        <SelectTrigger id="sched-format" className="h-10 w-36">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="pdf">{reportFormatLabel(t, "pdf")}</SelectItem>
                          <SelectItem value="xlsx">{reportFormatLabel(t, "xlsx")}</SelectItem>
                        </SelectContent>
                      </Select>
                    </Field>
                    <label className="flex h-10 items-center gap-2 text-sm">
                      <Switch
                        checked={enabled}
                        onCheckedChange={setEnabled}
                        aria-label={t("schedule.enabled")}
                      />
                      {t("schedule.enabled")}
                    </label>
                  </div>
                  <p className="text-xs text-text-muted">
                    {built.ok ? describe(spec) : null} {t("schedule.localTimeHint")}
                  </p>
                  {buildError ? (
                    <p role="alert" className="text-sm text-text-danger">
                      {buildError}
                    </p>
                  ) : null}
                  <Button
                    type="button"
                    className="h-10"
                    disabled={saving || templateId === null || !built.ok}
                    onClick={() => void onAdd()}
                  >
                    <Plus className="mr-1.5 h-4 w-4" aria-hidden />
                    {saving ? t("schedule.saving") : t("schedule.save")}
                  </Button>
                </>
              )}
            </fieldset>

            {schedules.length === 0 ? (
              <EmptyState
                icon={<CalendarClock className="h-6 w-6" aria-hidden />}
                title={t("schedule.empty")}
                description={t("schedule.emptyHint")}
              />
            ) : (
              <div className="overflow-x-auto rounded-md border border-surface-border">
                <table className="w-full border-collapse text-left text-sm">
                  <thead>
                    <tr className={cn("border-b border-surface-border", mfTable.header)}>
                      <th className="px-3 py-2">{t("schedule.template")}</th>
                      <th className="px-3 py-2">{t("schedule.when")}</th>
                      <th className="px-3 py-2">{t("schedule.format")}</th>
                      <th className="px-3 py-2">{t("schedule.next")}</th>
                      <th className="px-3 py-2">{t("schedule.last")}</th>
                      <th className="px-3 py-2">{t("schedule.enabled")}</th>
                      <th className="px-3 py-2">
                        <span className="sr-only">{t("schedule.delete")}</span>
                      </th>
                    </tr>
                  </thead>
                  <tbody>
                    {schedules.map((s) => {
                      const tpl = templateById.get(s.template_id);
                      const name = tpl ? reportTemplateTitle(t, tpl) : t("runs.unknownTemplate");
                      const parsed = parseScheduleCron(s.cron_expr, offset);
                      return (
                        <tr key={s.id} className="border-b border-surface-border/60 align-top">
                          <td className="px-3 py-2 text-text-primary">{name}</td>
                          <td className="px-3 py-2">
                            {parsed ? (
                              describe(parsed)
                            ) : (
                              <details className="text-xs text-text-muted">
                                <summary className="cursor-pointer select-none text-sm text-text-primary">
                                  {t("schedule.custom")}
                                </summary>
                                <p className="mt-1 font-mono">{s.cron_expr}</p>
                              </details>
                            )}
                          </td>
                          <td className="px-3 py-2">{reportFormatLabel(t, s.export_format)}</td>
                          <td className="whitespace-nowrap px-3 py-2 text-xs">
                            {s.enabled ? formatDateTime(s.next_run_at) : t("schedule.paused")}
                          </td>
                          <td className="whitespace-nowrap px-3 py-2 text-xs">
                            {s.last_run_at ? formatDateTime(s.last_run_at) : t("schedule.never")}
                          </td>
                          <td className="px-3 py-2">
                            <Switch
                              checked={s.enabled}
                              disabled={togglingId === s.id}
                              aria-label={t("schedule.toggleAria", { name })}
                              onCheckedChange={(next) => void onToggle(s, next)}
                            />
                          </td>
                          <td className="px-3 py-2 text-right">
                            <Button
                              type="button"
                              variant="ghost"
                              className="h-10"
                              onClick={() => setToDelete(s)}
                            >
                              <Trash2 className="mr-1.5 h-4 w-4" aria-hidden />
                              {t("schedule.delete")}
                            </Button>
                          </td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              </div>
            )}
          </>
        )}

        <ConfirmDialog
          open={toDelete !== null}
          onOpenChange={(open) => {
            if (!open) setToDelete(null);
          }}
          title={t("schedule.deleteTitle")}
          description={t("schedule.deleteDescription", {
            name: toDelete
              ? (() => {
                  const tpl = templateById.get(toDelete.template_id);
                  return tpl ? reportTemplateTitle(t, tpl) : t("runs.unknownTemplate");
                })()
              : "",
          })}
          confirmLabel={t("schedule.deleteConfirm")}
          destructive
          onConfirm={async () => {
            if (!toDelete) return;
            await deleteMyReportSchedule(toDelete.id);
            load();
          }}
        />
      </CardContent>
    </Card>
  );
}

function Field({ label, id, children }: { label: string; id: string; children: ReactNode }) {
  return (
    <div className="flex flex-col gap-1">
      <label htmlFor={id} className="text-xs text-text-muted">
        {label}
      </label>
      {children}
    </div>
  );
}
