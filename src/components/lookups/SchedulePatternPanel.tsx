/**
 * Classe horaire (ORG.SCHEDULE_CLASS) timetable — opened from the reference data editor.
 *
 * A Classe horaire is versioned (`personnel::timetable`): each version applies from a date and
 * describes a cycle (a week, or an N-day rotation from a start date) with each day's shift,
 * break and net hours, whether it runs continuously (3×8, 24/7: through the site's rest days
 * and holidays), and the utilisation factor — the share of shift time available for work orders.
 * A version in force is never changed: the planner adds one from today or later, so past
 * capacity and plans stay as they were. Versions created by the upgrade are flagged for review.
 */
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { EntityFormDialog, EntityFormFooter } from "@/components/entity-form";
import {
  MAX_CYCLE_DAYS,
  TIMETABLE_TEMPLATE_IDS,
  type TimetableDraft,
  WEEK_DAYS,
  isoWeekday,
  netMinutes,
  plusDaysIso,
  resizeCycle,
  templateDraft,
  timetableMetrics,
  validateTimetable,
} from "@/components/lookups/timetable-editor";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { StatusChip } from "@/components/ui/status-chip";
import { TimeInput } from "@/components/ui/time-input";
import { mfCard } from "@/design-system/tokens";
import { mfTone } from "@/design-system/tone";
import { usePermissions } from "@/hooks/use-permissions";
import { cn } from "@/lib/utils";
import {
  type ScheduleClassVersion,
  type ScheduleCycleDay,
  type VersionWorkOrderImpact,
  confirmScheduleClassReview,
  getScheduleClassVersionImpact,
  listScheduleClassVersions,
  listSiteWorkWeeks,
  upsertScheduleClassVersion,
} from "@/services/workforce-service";
import { formatOrgIpcError } from "@/utils/errors";
import type { ReferenceValue } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

function todayIso(): string {
  const d = new Date();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${d.getFullYear()}-${m}-${day}`;
}

/** Version in force on `day` (versions sorted oldest first). */
function versionOn(
  versions: ScheduleClassVersion[],
  day: string,
): ScheduleClassVersion | undefined {
  return [...versions].reverse().find((v) => v.effective_from <= day);
}

function draftFrom(v: ScheduleClassVersion): TimetableDraft {
  return {
    kind: v.rotation_start_date ? "rotation" : "week",
    cycleLength: v.cycle_length_days,
    rotationStart: v.rotation_start_date ?? todayIso(),
    days: v.days.map((d) => ({ ...d })),
    isContinuous: v.is_continuous,
  };
}

export interface SchedulePatternChromeState {
  saving: boolean;
  loading: boolean;
  canSave: boolean;
  save: () => void;
}

export interface ClasseHoraireEditorProps {
  value: ReferenceValue;
  canMutate: boolean;
  onSaved?: () => void;
  onChromeChange?: (chrome: SchedulePatternChromeState) => void;
}

export function ClasseHoraireEditor({
  value,
  canMutate,
  onSaved,
  onChromeChange,
}: ClasseHoraireEditorProps) {
  const { t, i18n } = useTranslation("personnel");
  const today = todayIso();
  const [versions, setVersions] = useState<ScheduleClassVersion[]>([]);
  const [siteRestDays, setSiteRestDays] = useState<number[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<unknown>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [editing, setEditing] = useState<ScheduleClassVersion | null>(null);
  const [effectiveFrom, setEffectiveFrom] = useState(today);
  const [utilisation, setUtilisation] = useState("");
  const [note, setNote] = useState("");
  const [draft, setDraft] = useState<TimetableDraft>(() => templateDraft("day_week", today));

  const weekdayFmt = useMemo(
    () => new Intl.DateTimeFormat(i18n.language, { weekday: "long", timeZone: "UTC" }),
    [i18n.language],
  );
  const dateFmt = useMemo(
    () => new Intl.DateTimeFormat(i18n.language, { dateStyle: "medium", timeZone: "UTC" }),
    [i18n.language],
  );
  const num = useMemo(
    () => new Intl.NumberFormat(i18n.language, { maximumFractionDigits: 1 }),
    [i18n.language],
  );

  // The site rest days are `list_site_work_weeks`, which the server opens to per.view only: the
  // timetable is read without them for other users (no refused call, no error in place of it).
  const { can, isLoading: permissionsLoading } = usePermissions();
  const canReadSiteWeeks = can(P.PER_VIEW);

  const load = useCallback(async () => {
    setLoading(true);
    setLoadError(null);
    try {
      const rows = await listScheduleClassVersions(value.id);
      setVersions(rows);
      let restDays: number[] = [];
      if (canReadSiteWeeks) {
        const allSites = (await listSiteWorkWeeks())
          .filter((w) => w.entity_id == null && w.effective_from <= today)
          .sort((a, b) => a.effective_from.localeCompare(b.effective_from));
        restDays = allSites.at(-1)?.weekend_days ?? [];
      }
      setSiteRestDays(restDays);
      const current = versionOn(rows, today) ?? rows.at(-1);
      if (current) {
        setDraft(draftFrom(current));
        setUtilisation(String(current.utilisation_percent));
      }
    } catch (e) {
      setLoadError(e);
    } finally {
      setLoading(false);
    }
  }, [today, value.id, canReadSiteWeeks]);

  useEffect(() => {
    if (!permissionsLoading) void load();
  }, [load, permissionsLoading]);

  const utilisationValue = utilisation.trim() === "" ? null : Number(utilisation);
  const issue = validateTimetable(draft, utilisationValue);
  const metrics = timetableMetrics(draft.days, utilisationValue);
  const toReview = versions.some((v) => v.review_status === "to_review");
  const dateOk = /^\d{4}-\d{2}-\d{2}$/.test(effectiveFrom) && effectiveFrom >= today;

  const startEdit = (v: ScheduleClassVersion | null) => {
    setEditing(v);
    setError(null);
    if (v) {
      setDraft(draftFrom(v));
      setEffectiveFrom(v.effective_from);
      setUtilisation(String(v.utilisation_percent));
      setNote(v.note ?? "");
    } else {
      setEffectiveFrom(today);
      setNote("");
    }
  };

  const save = useCallback(async () => {
    if (!canMutate || issue || !dateOk || utilisationValue == null) return;
    setSaving(true);
    setError(null);
    try {
      await upsertScheduleClassVersion({
        id: editing?.id ?? null,
        expected_row_version: editing?.row_version ?? null,
        reference_value_id: value.id,
        effective_from: effectiveFrom,
        cycle_length_days: draft.cycleLength,
        rotation_start_date: draft.kind === "rotation" ? draft.rotationStart : null,
        days: draft.days,
        is_continuous: draft.isContinuous,
        utilisation_percent: utilisationValue,
        note: note.trim() || null,
      });
      setEditing(null);
      await load();
      onSaved?.();
    } catch (e) {
      setError(formatOrgIpcError(e));
    } finally {
      setSaving(false);
    }
  }, [
    canMutate,
    dateOk,
    draft,
    editing,
    effectiveFrom,
    issue,
    load,
    note,
    onSaved,
    utilisationValue,
    value.id,
  ]);

  /** Work orders planned on the version being edited: confirmed before the save. */
  const [impact, setImpact] = useState<VersionWorkOrderImpact | null>(null);
  const requestSave = useCallback(async () => {
    if (!canMutate || issue || !dateOk || utilisationValue == null) return;
    if (editing?.id != null) {
      try {
        const found = await getScheduleClassVersionImpact(editing.id);
        if (found.work_orders > 0) {
          setImpact(found);
          return;
        }
      } catch (e) {
        setError(formatOrgIpcError(e));
        return;
      }
    }
    await save();
  }, [canMutate, dateOk, editing, issue, save, utilisationValue]);

  useEffect(() => {
    onChromeChange?.({
      saving,
      loading,
      canSave: canMutate && !saving && !loading && issue == null && dateOk,
      save: () => void requestSave(),
    });
  }, [canMutate, dateOk, issue, loading, onChromeChange, requestSave, saving]);

  const confirmReview = async () => {
    setSaving(true);
    setError(null);
    try {
      setVersions(await confirmScheduleClassReview(value.id));
    } catch (e) {
      setError(formatOrgIpcError(e));
    } finally {
      setSaving(false);
    }
  };

  const patchDay = (next: ScheduleCycleDay) =>
    setDraft((d) => ({
      ...d,
      days: d.days.map((x) => (x.cycle_day === next.cycle_day ? next : x)),
    }));

  const dayLabel = (cycleDay: number) => {
    if (draft.kind === "week") {
      // 2024-01-01 is a Monday: cycle day 1 … 7 = Monday … Sunday.
      return weekdayFmt.format(new Date(Date.UTC(2024, 0, cycleDay)));
    }
    const valid = /^\d{4}-\d{2}-\d{2}$/.test(draft.rotationStart);
    return t("timetable.editor.rotationDay", {
      day: cycleDay,
      date: valid
        ? dateFmt.format(new Date(`${plusDaysIso(draft.rotationStart, cycleDay - 1)}T00:00:00Z`))
        : "—",
    });
  };

  const worksOnSiteRestDay =
    !draft.isContinuous &&
    siteRestDays.length > 0 &&
    draft.days.some((d) => {
      if (d.is_rest_day) return false;
      const weekday =
        draft.kind === "week"
          ? d.cycle_day
          : isoWeekday(plusDaysIso(draft.rotationStart, d.cycle_day - 1));
      return siteRestDays.includes(weekday);
    });

  if (loading && versions.length === 0) return <LoadingState />;
  if (loadError) return <ErrorState error={loadError} onRetry={() => void load()} />;

  return (
    <div className="space-y-4">
      <ConfirmDialog
        open={impact != null}
        onOpenChange={(open) => {
          if (!open) setImpact(null);
        }}
        title={t("timetable.impact.title")}
        description={t("timetable.impact.description", {
          count: impact?.work_orders ?? 0,
          codes: impact?.codes.join(", ") ?? "",
        })}
        confirmLabel={t("timetable.impact.confirm")}
        onConfirm={async () => {
          setImpact(null);
          await save();
        }}
      />
      {toReview ? (
        <div role="status" className={cn("rounded-md border p-3 text-sm", mfTone.attention.panel)}>
          <p className="font-medium text-text-primary">{t("timetable.review.classTitle")}</p>
          <p className="mt-1 text-text-secondary">{t("timetable.review.classBody")}</p>
          {canMutate ? (
            <Button
              size="sm"
              variant="outline"
              className="mt-2"
              disabled={saving}
              onClick={() => void confirmReview()}
            >
              {t("timetable.review.confirm")}
            </Button>
          ) : null}
        </div>
      ) : null}

      <section className="space-y-2">
        <h3 className="text-sm font-semibold text-text-primary">{t("timetable.editor.history")}</h3>
        {versions.length === 0 ? (
          <p className="text-sm text-text-muted">{t("timetable.editor.noVersion")}</p>
        ) : (
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-surface-border text-left text-xs text-text-muted">
                <th className="py-1.5 pr-2 font-medium">{t("timetable.editor.from")}</th>
                <th className="py-1.5 pr-2 font-medium">{t("timetable.editor.cycle")}</th>
                <th className="py-1.5 pr-2 text-right font-medium">
                  {t("timetable.editor.netHours")}
                </th>
                <th className="py-1.5 pr-2 text-right font-medium">
                  {t("timetable.editor.utilisation")}
                </th>
                <th className="py-1.5 pr-2 font-medium">{t("timetable.editor.status")}</th>
                <th className="py-1.5 font-medium">
                  <span className="sr-only">{t("timetable.editor.edit")}</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {versions.map((v) => {
                const inForce = versionOn(versions, today)?.id === v.id;
                const planned = v.effective_from > today;
                return (
                  <tr key={v.id} className="border-b border-surface-border/60">
                    <td className="py-1.5 pr-2">
                      {dateFmt.format(new Date(`${v.effective_from}T00:00:00Z`))}
                    </td>
                    <td className="py-1.5 pr-2">
                      {v.rotation_start_date
                        ? t("timetable.editor.rotationOf", { days: v.cycle_length_days })
                        : t("timetable.editor.weekly")}
                      {v.is_continuous ? ` · ${t("timetable.editor.continuousShort")}` : ""}
                      {v.note ? (
                        <p className="mt-0.5 max-w-md text-xs text-text-muted">{v.note}</p>
                      ) : null}
                    </td>
                    <td className="py-1.5 pr-2 text-right tabular-nums">
                      {t("timetable.editor.hoursPerCycle", {
                        hours: num.format(v.net_hours_per_cycle),
                        days: v.working_days_per_cycle,
                      })}
                    </td>
                    <td className="py-1.5 pr-2 text-right tabular-nums">
                      {num.format(v.utilisation_percent)} %
                    </td>
                    <td className="py-1.5 pr-2">
                      <span className="flex flex-wrap gap-1">
                        <StatusChip
                          label={t(
                            inForce
                              ? "timetable.editor.inForce"
                              : planned
                                ? "timetable.editor.planned"
                                : "timetable.editor.past",
                          )}
                          tone={inForce ? "success" : planned ? "info" : "neutral"}
                          dot
                        />
                        {v.review_status === "to_review" ? (
                          <StatusChip label={t("timetable.review.chip")} tone="attention" dot />
                        ) : null}
                      </span>
                    </td>
                    <td className="py-1.5 text-right">
                      {planned && canMutate ? (
                        <Button size="sm" variant="ghost" onClick={() => startEdit(v)}>
                          {t("timetable.editor.edit")}
                        </Button>
                      ) : null}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
      </section>

      {canMutate ? (
        <section className={cn(mfCard.panelMuted, "space-y-3")}>
          <div className="flex flex-wrap items-center justify-between gap-2">
            <h3 className="text-sm font-semibold text-text-primary">
              {editing
                ? t("timetable.editor.editPlanned")
                : versions.length === 0
                  ? t("timetable.editor.firstVersion")
                  : t("timetable.editor.newVersion")}
            </h3>
            {editing ? (
              <Button size="sm" variant="ghost" onClick={() => startEdit(null)}>
                {t("timetable.editor.cancelEdit")}
              </Button>
            ) : null}
          </div>

          <div className="grid gap-3 sm:grid-cols-2">
            <div className="grid gap-1.5">
              <Label htmlFor="tt-from">{t("timetable.editor.effectiveFrom")}</Label>
              <Input
                id="tt-from"
                type="date"
                min={today}
                value={effectiveFrom}
                onChange={(e) => setEffectiveFrom(e.target.value)}
                className="h-9"
              />
              <p className="text-xs text-text-muted">{t("timetable.editor.effectiveFromHelp")}</p>
            </div>
            <div className="grid gap-1.5">
              <Label htmlFor="tt-util">{t("timetable.editor.utilisationLabel")}</Label>
              <Input
                id="tt-util"
                type="number"
                min={1}
                max={100}
                step={1}
                inputMode="decimal"
                placeholder={t("timetable.editor.utilisationPlaceholder")}
                value={utilisation}
                aria-describedby="tt-util-help"
                onChange={(e) => setUtilisation(e.target.value)}
                className="h-9 w-32"
              />
              <p id="tt-util-help" className="text-xs text-text-muted">
                {t("timetable.editor.utilisationHelp")}
              </p>
            </div>
          </div>

          <div className="flex items-start gap-2">
            <Checkbox
              id="tt-continuous"
              checked={draft.isContinuous}
              onCheckedChange={(v) => setDraft((d) => ({ ...d, isContinuous: v === true }))}
            />
            <div>
              <Label htmlFor="tt-continuous">{t("timetable.editor.continuous")}</Label>
              <p className="text-xs text-text-muted">{t("timetable.editor.continuousHelp")}</p>
            </div>
          </div>

          <div className="space-y-1.5">
            <p className="text-xs font-medium text-text-muted">{t("timetable.editor.templates")}</p>
            <div
              className="flex flex-wrap gap-1.5"
              role="group"
              aria-label={t("timetable.editor.templates")}
            >
              {TIMETABLE_TEMPLATE_IDS.map((id) => (
                <Button
                  key={id}
                  type="button"
                  size="sm"
                  variant="outline"
                  className="h-7 text-xs"
                  onClick={() => setDraft(templateDraft(id, draft.rotationStart))}
                >
                  {t(`timetable.editor.template.${id}`)}
                </Button>
              ))}
            </div>
          </div>

          <div className="flex flex-wrap items-end gap-3">
            <div className="flex gap-1.5" role="group" aria-label={t("timetable.editor.cycle")}>
              <Button
                type="button"
                size="sm"
                variant={draft.kind === "week" ? "default" : "outline"}
                aria-pressed={draft.kind === "week"}
                onClick={() =>
                  setDraft((d) => ({
                    ...d,
                    kind: "week",
                    cycleLength: WEEK_DAYS,
                    days: resizeCycle(d.days, WEEK_DAYS),
                  }))
                }
              >
                {t("timetable.editor.weekly")}
              </Button>
              <Button
                type="button"
                size="sm"
                variant={draft.kind === "rotation" ? "default" : "outline"}
                aria-pressed={draft.kind === "rotation"}
                onClick={() => setDraft((d) => ({ ...d, kind: "rotation" }))}
              >
                {t("timetable.editor.rotation")}
              </Button>
            </div>
            {draft.kind === "rotation" ? (
              <>
                <div className="grid gap-1.5">
                  <Label htmlFor="tt-cycle">{t("timetable.editor.cycleLength")}</Label>
                  <Input
                    id="tt-cycle"
                    type="number"
                    min={1}
                    max={MAX_CYCLE_DAYS}
                    value={draft.cycleLength}
                    onChange={(e) => {
                      const n = Number(e.target.value);
                      if (!Number.isFinite(n) || n < 1) return;
                      const length = Math.min(MAX_CYCLE_DAYS, Math.trunc(n));
                      setDraft((d) => ({
                        ...d,
                        cycleLength: length,
                        days: resizeCycle(d.days, length),
                      }));
                    }}
                    className="h-9 w-24"
                  />
                </div>
                <div className="grid gap-1.5">
                  <Label htmlFor="tt-rotation-start">{t("timetable.editor.rotationStart")}</Label>
                  <Input
                    id="tt-rotation-start"
                    type="date"
                    value={draft.rotationStart}
                    onChange={(e) => setDraft((d) => ({ ...d, rotationStart: e.target.value }))}
                    className="h-9"
                  />
                </div>
              </>
            ) : null}
          </div>

          <div className={cn(mfCard.insetCanvas, "overflow-x-auto p-2")}>
            <table className="w-full min-w-[34rem] text-sm">
              <thead>
                <tr className="text-left text-xs text-text-muted">
                  <th className="py-1 pr-2 font-medium">{t("timetable.editor.day")}</th>
                  <th className="py-1 pr-2 font-medium">{t("timetable.editor.restDay")}</th>
                  <th className="py-1 pr-2 font-medium">{t("timetable.editor.start")}</th>
                  <th className="py-1 pr-2 font-medium">{t("timetable.editor.end")}</th>
                  <th className="py-1 pr-2 font-medium">{t("timetable.editor.breakMinutes")}</th>
                  <th className="py-1 text-right font-medium">{t("timetable.editor.net")}</th>
                </tr>
              </thead>
              <tbody>
                {draft.days.map((d) => {
                  const label = dayLabel(d.cycle_day);
                  return (
                    <tr key={d.cycle_day} className={cn(d.is_rest_day && "text-text-muted")}>
                      <td className="py-1 pr-2 capitalize">{label}</td>
                      <td className="py-1 pr-2">
                        <Checkbox
                          checked={d.is_rest_day}
                          aria-label={t("timetable.editor.restDayOf", { day: label })}
                          onCheckedChange={(v) => patchDay({ ...d, is_rest_day: v === true })}
                        />
                      </td>
                      <td className="py-1 pr-2">
                        <TimeInput
                          value={d.shift_start}
                          disabled={d.is_rest_day}
                          aria-label={t("timetable.editor.startOf", { day: label })}
                          className="h-8 w-[7.5rem]"
                          onChange={(v) => patchDay({ ...d, shift_start: v })}
                        />
                      </td>
                      <td className="py-1 pr-2">
                        <TimeInput
                          value={d.shift_end}
                          disabled={d.is_rest_day}
                          aria-label={t("timetable.editor.endOf", { day: label })}
                          className="h-8 w-[7.5rem]"
                          onChange={(v) => patchDay({ ...d, shift_end: v })}
                        />
                      </td>
                      <td className="py-1 pr-2">
                        <Input
                          type="number"
                          min={0}
                          step={5}
                          value={d.break_minutes}
                          disabled={d.is_rest_day}
                          aria-label={t("timetable.editor.breakOf", { day: label })}
                          className="h-8 w-20"
                          onChange={(e) => {
                            const n = Number(e.target.value);
                            patchDay({
                              ...d,
                              break_minutes: Number.isFinite(n) && n >= 0 ? Math.trunc(n) : 0,
                            });
                          }}
                        />
                      </td>
                      <td className="py-1 text-right tabular-nums">
                        {d.is_rest_day ? "—" : `${num.format(netMinutes(d) / 60)} h`}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>

          <dl className="grid grid-cols-2 gap-2 text-sm sm:grid-cols-3">
            <div>
              <dt className="text-xs text-text-muted">{t("timetable.editor.workingDays")}</dt>
              <dd className="font-semibold">{metrics.workingDays}</dd>
            </div>
            <div>
              <dt className="text-xs text-text-muted">{t("timetable.editor.netHoursCycle")}</dt>
              <dd className="font-semibold">{num.format(metrics.netHoursPerCycle)} h</dd>
            </div>
            <div>
              <dt className="text-xs text-text-muted">
                {t("timetable.editor.plannableHoursCycle")}
              </dt>
              <dd className="font-semibold">
                {metrics.plannableHoursPerCycle == null
                  ? "—"
                  : `${num.format(metrics.plannableHoursPerCycle)} h`}
              </dd>
            </div>
          </dl>

          {worksOnSiteRestDay ? (
            <p className={cn("rounded border p-2 text-xs", mfTone.info.panel)} role="note">
              {t("timetable.editor.siteRestWarning", {
                days: siteRestDays
                  .map((wd) => weekdayFmt.format(new Date(Date.UTC(2024, 0, wd))))
                  .join(", "),
              })}
            </p>
          ) : null}

          <div className="grid gap-1.5">
            <Label htmlFor="tt-note">{t("timetable.editor.note")}</Label>
            <Input
              id="tt-note"
              value={note}
              onChange={(e) => setNote(e.target.value)}
              className="h-9"
            />
          </div>

          {issue ? (
            <p className={cn("text-xs", mfTone.danger.text)} role="alert">
              {t(`timetable.editor.issues.${issue}`)}
            </p>
          ) : !dateOk ? (
            <p className={cn("text-xs", mfTone.danger.text)} role="alert">
              {t("timetable.editor.issues.date")}
            </p>
          ) : null}
        </section>
      ) : (
        <p className="text-xs text-text-muted">{t("timetable.editor.readOnly")}</p>
      )}

      {error ? (
        <p className={cn("text-xs", mfTone.danger.text)} role="alert">
          {error}
        </p>
      ) : null}
    </div>
  );
}

interface SchedulePatternDialogProps {
  value: ReferenceValue | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  canMutate: boolean;
  onSaved?: () => void;
}

/** Wide entity dialog hosting a Classe horaire's timetable. */
export function SchedulePatternDialog({
  value,
  open,
  onOpenChange,
  canMutate,
  onSaved,
}: SchedulePatternDialogProps) {
  const { t } = useTranslation("personnel");
  const [chrome, setChrome] = useState<SchedulePatternChromeState>({
    saving: false,
    loading: true,
    canSave: false,
    save: () => undefined,
  });

  const handleChromeChange = useCallback((next: SchedulePatternChromeState) => {
    setChrome(next);
  }, []);

  if (!value) return null;

  return (
    <EntityFormDialog
      open={open}
      onOpenChange={onOpenChange}
      size="wide"
      title={t("timetable.editor.title", { name: value.label })}
      description={t("timetable.editor.description")}
      footer={
        <EntityFormFooter
          cancelLabel={t("timetable.editor.close")}
          primaryLabel={chrome.saving ? t("timetable.editor.saving") : t("timetable.editor.save")}
          onCancel={() => onOpenChange(false)}
          onPrimary={() => chrome.save()}
          primaryType="button"
          cancelDisabled={chrome.saving}
          primaryDisabled={!chrome.canSave}
          primaryLoading={chrome.saving}
        />
      }
    >
      <ClasseHoraireEditor
        key={value.id}
        value={value}
        canMutate={canMutate}
        onChromeChange={handleChromeChange}
        {...(onSaved ? { onSaved } : {})}
      />
    </EntityFormDialog>
  );
}

/** @deprecated Prefer SchedulePatternDialog — kept for import stability. */
export const SchedulePatternPanel = SchedulePatternDialog;
