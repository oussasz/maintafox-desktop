/**
 * Recurring rounds (PRD §6.25, Phase 6.3): schedules list and editor. A schedule walks one
 * checklist (its ordered, equipment-bound checkpoints are the route) every calendar interval or
 * every meter interval, and generates rounds assigned to a person and/or a team.
 */
import type { ColumnDef } from "@tanstack/react-table";
import { CalendarClock, Gauge, Loader2, Pencil } from "lucide-react";
import { type FormEvent, useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { AssetPicker } from "@/components/assets/AssetPicker";
import { LinkedEntityBadge } from "@/components/common/LinkedEntityBadge";
import { DataTable } from "@/components/data/DataTable";
import {
  Button,
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  FormField,
  Input,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui";
import { Checkbox } from "@/components/ui/checkbox";
import { ErrorState } from "@/components/ui/data-state";
import { DateTimeLocalField } from "@/components/ui/date-time-local-field";
import { EmptyState } from "@/components/ui/empty-state";
import { StatusChip } from "@/components/ui/status-chip";
import { useFormatters } from "@/hooks/use-formatters";
import { usePermissions } from "@/hooks/use-permissions";
import { listAssetMeters } from "@/services/asset-lifecycle-service";
import {
  listInspectionSchedules,
  listInspectionTemplates,
  saveInspectionSchedule,
} from "@/services/inspection-service";
import type { AssetMeter, InspectionScheduleSummary, InspectionTemplate } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

import { PersonnelPicker, TeamSelect } from "./InspectionRoundsPanel";
import {
  INTERVAL_UNITS,
  MAX_GRACE_HOURS,
  MAX_HORIZON_DAYS,
  type ScheduleForm,
  isoToLocalInput,
  parseReading,
  scheduleFormIssues,
} from "./inspection-rules";

interface InspectionSchedulesPanelProps {
  refreshKey: number;
  onEdit: (schedule: InspectionScheduleSummary) => void;
  onCountChange?: (total: number | null) => void;
}

export function InspectionSchedulesPanel({
  refreshKey,
  onEdit,
  onCountChange,
}: InspectionSchedulesPanelProps) {
  const { t } = useTranslation("inspections");
  const { can } = usePermissions();
  const { formatDateTime, formatNumber } = useFormatters();
  const [rows, setRows] = useState<InspectionScheduleSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<unknown>(null);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const all = await listInspectionSchedules();
      setRows(all);
      setError(null);
      onCountChange?.(all.length);
    } catch (err) {
      setError(err);
      onCountChange?.(null);
    } finally {
      setLoading(false);
    }
  }, [onCountChange]);

  useEffect(() => {
    void load();
  }, [load, refreshKey]);

  const manage = can(P.INS_ADMIN);

  const columns = useMemo<ColumnDef<InspectionScheduleSummary>[]>(
    () => [
      {
        id: "schedule",
        accessorFn: (r) => r.schedule.code,
        header: t("schedules.columns.schedule"),
        cell: ({ row }) => (
          <div className="min-w-0">
            <p className="font-medium text-text-primary">{row.original.schedule.name}</p>
            <p className="font-mono text-xs text-text-muted">{row.original.schedule.code}</p>
          </div>
        ),
      },
      {
        id: "route",
        accessorFn: (r) => r.template_name,
        header: t("schedules.columns.route"),
        cell: ({ row }) => (
          <span className="text-sm">
            {row.original.template_name}{" "}
            <span className="font-mono text-xs text-text-muted">
              ({row.original.template_code})
            </span>
          </span>
        ),
      },
      {
        id: "trigger",
        accessorFn: (r) => r.schedule.trigger_kind,
        header: t("schedules.columns.trigger"),
        cell: ({ row }) => {
          const r = row.original;
          const s = r.schedule;
          if (s.trigger_kind === "meter") {
            return (
              <div className="flex flex-col gap-1 text-xs">
                <span className="flex items-center gap-1">
                  <Gauge className="h-3.5 w-3.5" aria-hidden />
                  {t("schedules.everyMeter", {
                    value: formatNumber(s.meter_interval ?? 0),
                    unit: r.meter_unit ?? "",
                    meter: r.meter_name ?? "",
                  })}
                </span>
                <span className="flex flex-wrap items-center gap-1 text-text-muted">
                  <LinkedEntityBadge
                    entity="asset"
                    code={r.meter_asset_code}
                    entityId={r.meter_asset_id}
                  />
                  {r.meter_lifetime_reading != null
                    ? t("schedules.meterNow", {
                        value: formatNumber(r.meter_lifetime_reading),
                        unit: r.meter_unit ?? "",
                      })
                    : null}
                </span>
              </div>
            );
          }
          return (
            <div className="flex flex-col gap-0.5 text-xs">
              <span className="flex items-center gap-1">
                <CalendarClock className="h-3.5 w-3.5" aria-hidden />
                {t(`schedules.every.${s.interval_unit ?? "day"}`, { count: s.interval_value ?? 0 })}
              </span>
              <span className="text-text-muted">
                {t("schedules.from", { date: formatDateTime(s.anchor_at) })}
              </span>
            </div>
          );
        },
      },
      {
        id: "assignee",
        accessorFn: (r) => r.assigned_to_name ?? r.assigned_team_name ?? "",
        header: t("schedules.columns.assignee"),
        cell: ({ row }) => (
          <div className="text-xs">
            {row.original.assigned_to_name ? <p>{row.original.assigned_to_name}</p> : null}
            {row.original.assigned_team_name ? (
              <p className="text-text-muted">
                {t("rounds.team", { name: row.original.assigned_team_name })}
              </p>
            ) : null}
          </div>
        ),
      },
      {
        id: "next",
        accessorFn: (r) => r.next_due_at ?? "",
        header: t("schedules.columns.next"),
        cell: ({ row }) => (
          <span className="text-xs">
            {row.original.next_due_at ? formatDateTime(row.original.next_due_at) : "—"}
          </span>
        ),
      },
      {
        id: "state",
        accessorFn: (r) => r.overdue_round_count + r.missed_round_count,
        header: t("schedules.columns.rounds"),
        cell: ({ row }) => {
          const r = row.original;
          return (
            <div className="flex flex-wrap gap-1">
              <StatusChip label={t("schedules.open", { count: r.open_round_count })} tone="info" />
              {r.overdue_round_count > 0 ? (
                <StatusChip
                  label={t("schedules.overdue", { count: r.overdue_round_count })}
                  tone="danger"
                />
              ) : null}
              {r.missed_round_count > 0 ? (
                <StatusChip
                  label={t("schedules.missed", { count: r.missed_round_count })}
                  tone="danger"
                />
              ) : null}
            </div>
          );
        },
      },
      {
        id: "active",
        accessorFn: (r) => (r.schedule.is_active ? 1 : 0),
        header: t("schedules.columns.active"),
        cell: ({ row }) => (
          <StatusChip
            label={
              row.original.schedule.is_active ? t("schedules.active") : t("schedules.inactive")
            }
            tone={row.original.schedule.is_active ? "success" : "neutral"}
          />
        ),
      },
      ...(manage
        ? [
            {
              id: "edit",
              enableSorting: false,
              header: () => <span className="sr-only">{t("schedules.edit")}</span>,
              cell: ({ row }: { row: { original: InspectionScheduleSummary } }) => (
                <Button
                  size="sm"
                  variant="outline"
                  className="h-11"
                  aria-label={t("schedules.editNamed", { name: row.original.schedule.name })}
                  onClick={(e) => {
                    e.stopPropagation();
                    onEdit(row.original);
                  }}
                >
                  <Pencil className="mr-1.5 h-4 w-4" aria-hidden />
                  {t("schedules.edit")}
                </Button>
              ),
            } satisfies ColumnDef<InspectionScheduleSummary>,
          ]
        : []),
    ],
    [formatDateTime, formatNumber, manage, onEdit, t],
  );

  if (error != null && rows.length === 0) {
    return <ErrorState error={error} onRetry={() => void load()} retrying={loading} />;
  }
  if (!loading && rows.length === 0) {
    return <EmptyState title={t("schedules.empty")} description={t("schedules.emptyHint")} />;
  }
  return (
    <div data-testid="inspection-schedule-list">
      <DataTable columns={columns} data={rows} pageSize={20} isLoading={loading} skeletonRows={4} />
    </div>
  );
}

function emptyForm(): ScheduleForm {
  return {
    code: "",
    name: "",
    templateId: "",
    triggerKind: "calendar",
    intervalValue: "1",
    intervalUnit: "day",
    anchorLocal: "",
    meterId: null,
    meterInterval: "",
    meterStart: "",
    assigneeId: null,
    teamId: null,
    graceHours: "24",
    horizonDays: "14",
  };
}

function formFrom(s: InspectionScheduleSummary): ScheduleForm {
  const x = s.schedule;
  return {
    code: x.code,
    name: x.name,
    templateId: String(x.template_id),
    triggerKind: x.trigger_kind === "meter" ? "meter" : "calendar",
    intervalValue: x.interval_value != null ? String(x.interval_value) : "",
    intervalUnit: x.interval_unit ?? "day",
    anchorLocal: isoToLocalInput(x.anchor_at),
    meterId: x.meter_id,
    meterInterval: x.meter_interval != null ? String(x.meter_interval) : "",
    meterStart: x.meter_start_reading != null ? String(x.meter_start_reading) : "",
    assigneeId: x.assigned_to_id,
    teamId: x.assigned_team_id,
    graceHours: String(x.grace_hours),
    horizonDays: String(x.horizon_days),
  };
}

interface InspectionScheduleDialogProps {
  /** `null`: closed; `"new"`: create; a schedule: edit it. */
  target: InspectionScheduleSummary | "new" | null;
  onClose: () => void;
  onSaved: () => void;
}

export function InspectionScheduleDialog({
  target,
  onClose,
  onSaved,
}: InspectionScheduleDialogProps) {
  const { t } = useTranslation("inspections");
  const existing = target != null && target !== "new" ? target : null;
  const [form, setForm] = useState<ScheduleForm>(emptyForm);
  const [active, setActive] = useState(true);
  const [assigneeName, setAssigneeName] = useState<string | null>(null);
  const [templates, setTemplates] = useState<InspectionTemplate[]>([]);
  const [asset, setAsset] = useState<{ id: number; asset_code: string; asset_name: string } | null>(
    null,
  );
  const [meters, setMeters] = useState<AssetMeter[]>([]);
  /** The asset's meters could not be read: said so, never shown as "no meter". */
  const [metersError, setMetersError] = useState<unknown>(null);
  const [metersReload, setMetersReload] = useState(0);
  const [loadError, setLoadError] = useState<unknown>(null);
  const [submitted, setSubmitted] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (target == null) return;
    setSubmitted(false);
    setLoadError(null);
    if (existing) {
      setForm(formFrom(existing));
      setActive(existing.schedule.is_active);
      setAssigneeName(existing.assigned_to_name);
      setAsset(
        existing.meter_asset_id != null
          ? {
              id: existing.meter_asset_id,
              asset_code: existing.meter_asset_code ?? "",
              asset_name: "",
            }
          : null,
      );
    } else {
      setForm(emptyForm());
      setActive(true);
      setAssigneeName(null);
      setAsset(null);
    }
    listInspectionTemplates()
      .then((all) => setTemplates(all.filter((x) => x.is_active && x.current_version_id != null)))
      .catch((err: unknown) => setLoadError(err));
  }, [target, existing]);

  useEffect(() => {
    setMetersError(null);
    if (!asset) {
      setMeters([]);
      return;
    }
    listAssetMeters(asset.id)
      .then((all) => setMeters(all.filter((m) => m.is_active)))
      .catch((err: unknown) => {
        setMeters([]);
        setMetersError(err);
      });
  }, [asset, metersReload]);

  const issues = scheduleFormIssues(form);
  const err = (field: keyof typeof issues) =>
    submitted && issues[field]
      ? t(`schedules.errors.${issues[field]}`, { hours: MAX_GRACE_HOURS, days: MAX_HORIZON_DAYS })
      : undefined;
  const set = (patch: Partial<ScheduleForm>) => setForm((f) => ({ ...f, ...patch }));

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setSubmitted(true);
    if (Object.keys(issues).length > 0) return;
    setBusy(true);
    try {
      const calendar = form.triggerKind === "calendar";
      await saveInspectionSchedule({
        id: existing?.schedule.id ?? null,
        expected_row_version: existing?.schedule.row_version ?? null,
        code: form.code.trim(),
        name: form.name.trim(),
        template_id: Number(form.templateId),
        trigger_kind: form.triggerKind,
        interval_unit: calendar ? form.intervalUnit : null,
        interval_value: calendar ? Number(form.intervalValue) : null,
        anchor_at: calendar ? new Date(form.anchorLocal).toISOString() : null,
        meter_id: calendar ? null : form.meterId,
        meter_interval: calendar ? null : parseReading(form.meterInterval),
        meter_start_reading:
          calendar || form.meterStart.trim() === "" ? null : parseReading(form.meterStart),
        assigned_to_id: form.assigneeId,
        assigned_team_id: form.teamId,
        grace_hours: Number(form.graceHours),
        horizon_days: Number(form.horizonDays),
        is_active: active,
      });
      onSaved();
    } catch {
      // The global IPC toast reports the failure (gate codes are translated); stay open.
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={target != null} onOpenChange={(open) => (!open ? onClose() : undefined)}>
      <DialogContent className="max-h-[90vh] max-w-2xl overflow-y-auto">
        <form onSubmit={(e) => void submit(e)} className="space-y-4" noValidate>
          <DialogHeader>
            <DialogTitle>
              {existing
                ? t("schedules.editTitle", { name: existing.schedule.name })
                : t("schedules.newTitle")}
            </DialogTitle>
            <DialogDescription>{t("schedules.intro")}</DialogDescription>
          </DialogHeader>
          {loadError != null ? <ErrorState error={loadError} /> : null}

          <div className="grid gap-3 md:grid-cols-2">
            <FormField name="sched-code" label={t("schedules.code")} required error={err("code")}>
              <Input
                id="sched-code"
                value={form.code}
                onChange={(e) => set({ code: e.target.value })}
              />
            </FormField>
            <FormField name="sched-name" label={t("schedules.name")} required error={err("name")}>
              <Input
                id="sched-name"
                value={form.name}
                onChange={(e) => set({ name: e.target.value })}
              />
            </FormField>
          </div>

          <FormField
            name="sched-template"
            label={t("schedules.route")}
            description={t("schedules.routeHint")}
            required
            error={err("templateId")}
          >
            <Select value={form.templateId} onValueChange={(v) => set({ templateId: v })}>
              <SelectTrigger id="sched-template" className="h-11">
                <SelectValue placeholder={t("schedule.templatePlaceholder")} />
              </SelectTrigger>
              <SelectContent>
                {templates.map((x) => (
                  <SelectItem key={x.id} value={String(x.id)}>
                    {x.name} ({x.code})
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </FormField>

          <fieldset className="space-y-2">
            <legend className="text-sm font-medium">{t("schedules.trigger")}</legend>
            <div
              role="radiogroup"
              aria-label={t("schedules.trigger")}
              className="flex flex-wrap gap-2"
            >
              {(["calendar", "meter"] as const).map((k) => (
                <button
                  key={k}
                  type="button"
                  role="radio"
                  aria-checked={form.triggerKind === k}
                  onClick={() => set({ triggerKind: k })}
                  className={
                    form.triggerKind === k
                      ? "min-h-11 rounded-md border-2 border-primary bg-primary/10 px-4 text-sm font-medium focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                      : "min-h-11 rounded-md border-2 border-surface-border px-4 text-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                  }
                >
                  {t(`schedules.triggers.${k}`)}
                </button>
              ))}
            </div>
          </fieldset>

          {form.triggerKind === "calendar" ? (
            <div className="grid gap-3 md:grid-cols-3">
              <FormField
                name="sched-every"
                label={t("schedules.every.label")}
                required
                error={err("intervalValue")}
              >
                <Input
                  id="sched-every"
                  inputMode="numeric"
                  value={form.intervalValue}
                  onChange={(e) => set({ intervalValue: e.target.value })}
                />
              </FormField>
              <FormField name="sched-unit" label={t("schedules.unit")} required>
                <Select value={form.intervalUnit} onValueChange={(v) => set({ intervalUnit: v })}>
                  <SelectTrigger id="sched-unit" className="h-11">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {INTERVAL_UNITS.map((u) => (
                      <SelectItem key={u} value={u}>
                        {t(`schedules.units.${u}`)}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </FormField>
              <FormField
                name="sched-horizon"
                label={t("schedules.horizon")}
                description={t("schedules.horizonHint", { max: MAX_HORIZON_DAYS })}
                required
                error={err("horizonDays")}
              >
                <Input
                  id="sched-horizon"
                  inputMode="numeric"
                  value={form.horizonDays}
                  onChange={(e) => set({ horizonDays: e.target.value })}
                />
              </FormField>
              <div className="md:col-span-3">
                <DateTimeLocalField
                  id="sched-anchor"
                  label={t("schedules.anchor")}
                  value={form.anchorLocal}
                  onChange={(v) => set({ anchorLocal: v })}
                />
                {err("anchorLocal") ? (
                  <p role="alert" className="mt-1 text-xs text-tone-danger-fg">
                    {err("anchorLocal")}
                  </p>
                ) : null}
              </div>
            </div>
          ) : (
            <div className="space-y-3">
              <FormField name="sched-asset" label={t("schedules.meterAsset")} required>
                {asset ? (
                  <div className="flex min-h-11 items-center justify-between rounded-md border border-surface-border px-3 text-sm">
                    <span>
                      <span className="font-mono">{asset.asset_code}</span> {asset.asset_name}
                    </span>
                    <Button
                      type="button"
                      variant="ghost"
                      className="h-11"
                      onClick={() => {
                        setAsset(null);
                        set({ meterId: null });
                      }}
                    >
                      {t("schedule.clearAssignee")}
                    </Button>
                  </div>
                ) : (
                  <AssetPicker
                    id="sched-asset"
                    value={null}
                    onChange={(a) =>
                      setAsset(
                        a ? { id: a.id, asset_code: a.asset_code, asset_name: a.asset_name } : null,
                      )
                    }
                  />
                )}
              </FormField>
              <FormField
                name="sched-meter"
                label={t("schedules.meter")}
                required
                error={err("meterId")}
              >
                {metersError != null ? (
                  <ErrorState
                    className="py-4"
                    title={t("schedules.metersLoadError")}
                    error={metersError}
                    onRetry={() => setMetersReload((k) => k + 1)}
                  />
                ) : asset && meters.length === 0 ? (
                  <p className="text-sm text-text-muted">{t("schedules.noMeter")}</p>
                ) : (
                  <Select
                    value={form.meterId != null ? String(form.meterId) : ""}
                    onValueChange={(v) => set({ meterId: Number(v) })}
                    disabled={!asset}
                  >
                    <SelectTrigger id="sched-meter" className="h-11">
                      <SelectValue placeholder={t("schedules.meterPlaceholder")} />
                    </SelectTrigger>
                    <SelectContent>
                      {meters.map((m) => (
                        <SelectItem key={m.id} value={String(m.id)}>
                          {m.name}
                          {m.unit ? ` (${m.unit})` : ""}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                )}
              </FormField>
              <div className="grid gap-3 md:grid-cols-2">
                <FormField
                  name="sched-meter-interval"
                  label={t("schedules.meterInterval")}
                  required
                  error={err("meterInterval")}
                >
                  <Input
                    id="sched-meter-interval"
                    inputMode="decimal"
                    value={form.meterInterval}
                    onChange={(e) => set({ meterInterval: e.target.value })}
                  />
                </FormField>
                <FormField
                  name="sched-meter-start"
                  label={t("schedules.meterStart")}
                  description={t("schedules.meterStartHint")}
                  error={err("meterStart")}
                >
                  <Input
                    id="sched-meter-start"
                    inputMode="decimal"
                    value={form.meterStart}
                    onChange={(e) => set({ meterStart: e.target.value })}
                  />
                </FormField>
              </div>
            </div>
          )}

          <div className="grid gap-3 md:grid-cols-2">
            <PersonnelPicker
              id="sched-assignee"
              value={
                form.assigneeId != null
                  ? { id: form.assigneeId, full_name: assigneeName ?? "" }
                  : null
              }
              onChange={(p) => {
                set({ assigneeId: p?.id ?? null });
                setAssigneeName(p?.full_name ?? null);
              }}
            />
            <TeamSelect
              id="sched-team"
              value={form.teamId}
              onChange={(id) => set({ teamId: id })}
            />
          </div>
          {err("assignee") ? (
            <p role="alert" className="text-xs text-tone-danger-fg">
              {err("assignee")}
            </p>
          ) : null}

          <FormField
            name="sched-grace"
            label={t("schedules.grace")}
            description={t("schedules.graceHint", { max: MAX_GRACE_HOURS })}
            required
            error={err("graceHours")}
          >
            <Input
              id="sched-grace"
              inputMode="numeric"
              className="max-w-40"
              value={form.graceHours}
              onChange={(e) => set({ graceHours: e.target.value })}
            />
          </FormField>

          <label className="flex min-h-11 items-center gap-2 text-sm">
            <Checkbox checked={active} onCheckedChange={(v) => setActive(v === true)} />
            {t("schedules.activeLabel")}
          </label>

          <DialogFooter>
            <Button type="button" variant="outline" className="h-11" onClick={onClose}>
              {t("followUp.cancel")}
            </Button>
            <Button type="submit" className="h-11" disabled={busy}>
              {busy ? <Loader2 className="mr-2 h-4 w-4 animate-spin" aria-hidden /> : null}
              {t("schedules.save")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
