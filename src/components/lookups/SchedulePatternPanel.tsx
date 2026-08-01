/**
 * SchedulePatternEditor — weekly calendar body for ORG.SCHEDULE_CLASS.
 * Hosted in EntityFormDialog (wide) via SchedulePatternDialog.
 * Storage/IPC unchanged: getSchedulePattern / upsertSchedulePattern.
 */

import { useCallback, useEffect, useMemo, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";

import {
  applyTemplate,
  computeMetrics,
  DAY_KEYS,
  dayDurationHours,
  defaultEditorDays,
  fromScheduleDetails,
  intervalBarSegments,
  SCHEDULE_TEMPLATE_IDS,
  toScheduleDetails,
  updateDayInterval,
  updateDayMode,
  validateEditorDays,
  type DayOfWeek,
  type EditorDay,
  type ScheduleTemplateId,
} from "@/components/lookups/schedule-pattern-editor";
import {
  EntityFormDialog,
  EntityFormFooter,
} from "@/components/entity-form";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { TimeInput } from "@/components/ui/time-input";
import { mfCard } from "@/design-system/tokens";
import { cn } from "@/lib/utils";
import {
  getSchedulePattern,
  upsertSchedulePattern,
} from "@/services/reference-service";
import { toErrorMessage } from "@/utils/errors";
import type { ReferenceValue } from "@shared/ipc-types";

export interface SchedulePatternEditorProps {
  value: ReferenceValue;
  canMutate: boolean;
  onSaved?: () => void;
  /** Called when save completes successfully so the dialog can close. */
  onRequestClose?: () => void;
  /** Expose chrome state to the dialog footer. */
  onChromeChange?: (chrome: SchedulePatternChromeState) => void;
}

export interface SchedulePatternChromeState {
  saving: boolean;
  loading: boolean;
  canSave: boolean;
  save: () => void;
}

export function SchedulePatternEditor({
  value,
  canMutate,
  onSaved,
  onRequestClose,
  onChromeChange,
}: SchedulePatternEditorProps) {
  const { t } = useTranslation("reference");
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [shiftPatternCode, setShiftPatternCode] = useState(value.code);
  const [isContinuous, setIsContinuous] = useState(false);
  const [days, setDays] = useState<EditorDay[]>(defaultEditorDays);
  const [selectedDay, setSelectedDay] = useState<DayOfWeek>(1);
  const [activeTemplate, setActiveTemplate] = useState<ScheduleTemplateId | null>("normal_day");

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const pattern = await getSchedulePattern(value.id);
      setShiftPatternCode(pattern.shift_pattern_code);
      setIsContinuous(pattern.is_continuous);
      setDays(
        pattern.details.length === 7
          ? fromScheduleDetails(pattern.details)
          : defaultEditorDays(),
      );
      setActiveTemplate("personalized");
      setSelectedDay(1);
    } catch (e) {
      setError(toErrorMessage(e));
      setDays(defaultEditorDays());
    } finally {
      setLoading(false);
    }
  }, [value.id]);

  useEffect(() => {
    void load();
  }, [load]);

  const metrics = useMemo(
    () => computeMetrics(days, selectedDay),
    [days, selectedDay],
  );
  const validation = useMemo(() => validateEditorDays(days), [days]);

  const markPersonalized = () => setActiveTemplate("personalized");

  const handleApplyTemplate = (id: ScheduleTemplateId) => {
    if (!canMutate) return;
    if (id === "personalized") {
      setActiveTemplate("personalized");
      return;
    }
    const applied = applyTemplate(id);
    setDays(applied.days);
    setIsContinuous(applied.isContinuous);
    setActiveTemplate(id);
    setError(null);
  };

  const patchDay = (next: EditorDay) => {
    setDays((prev) => prev.map((d) => (d.dayOfWeek === next.dayOfWeek ? next : d)));
    markPersonalized();
  };

  const handleSave = useCallback(async () => {
    if (!canMutate) return;
    const check = validateEditorDays(days);
    if (!check.valid) {
      setError(
        check.errorKey
          ? String(t(`schedulePattern.validation.${check.errorKey}`))
          : String(t("schedulePattern.validation.generic")),
      );
      return;
    }
    setSaving(true);
    setError(null);
    try {
      await upsertSchedulePattern({
        reference_value_id: value.id,
        shift_pattern_code: shiftPatternCode.trim() || value.code,
        is_continuous: isContinuous,
        nominal_hours_per_day: metrics.nominalHoursPerDay || 8,
        details: toScheduleDetails(days),
      });
      onSaved?.();
      onRequestClose?.();
    } catch (e) {
      setError(toErrorMessage(e));
    } finally {
      setSaving(false);
    }
  }, [
    canMutate,
    days,
    isContinuous,
    metrics.nominalHoursPerDay,
    onRequestClose,
    onSaved,
    shiftPatternCode,
    t,
    value.code,
    value.id,
  ]);

  const canSave = canMutate && !saving && !loading && validation.valid;

  useEffect(() => {
    onChromeChange?.({
      saving,
      loading,
      canSave,
      save: () => {
        void handleSave();
      },
    });
  }, [canSave, handleSave, loading, onChromeChange, saving]);

  const onWeekKeyDown = (e: KeyboardEvent) => {
    if (e.key === "ArrowRight" || e.key === "ArrowDown") {
      e.preventDefault();
      setSelectedDay((d) => (d === 7 ? 1 : ((d + 1) as DayOfWeek)));
    } else if (e.key === "ArrowLeft" || e.key === "ArrowUp") {
      e.preventDefault();
      setSelectedDay((d) => (d === 1 ? 7 : ((d - 1) as DayOfWeek)));
    }
  };

  if (loading) {
    return <p className="text-sm text-text-muted">{t("schedulePattern.loading")}</p>;
  }

  return (
    <div className="space-y-4">
      <div className={cn(mfCard.panelMuted, "grid grid-cols-2 gap-3 sm:grid-cols-4")}>
        <Metric
          label={t("schedulePattern.metrics.workingDays")}
          value={String(metrics.workingDays)}
        />
        <Metric
          label={t("schedulePattern.metrics.restDays")}
          value={String(metrics.restDays)}
        />
        <Metric
          label={t("schedulePattern.metrics.weeklyHours")}
          value={t("schedulePattern.metrics.hoursValue", {
            hours: metrics.weeklyHours,
          })}
        />
        <Metric
          label={t("schedulePattern.metrics.dayDuration")}
          value={
            days.find((d) => d.dayOfWeek === selectedDay)?.mode === "rest"
              ? t("schedulePattern.restDayLabel")
              : t("schedulePattern.metrics.hoursValue", {
                  hours: metrics.selectedDayHours,
                })
          }
        />
      </div>

      <div className="grid gap-3 sm:grid-cols-2">
        <div className="grid gap-1.5">
          <Label htmlFor="shift-pattern-code">{t("schedulePattern.shiftPatternCode")}</Label>
          <Input
            id="shift-pattern-code"
            value={shiftPatternCode}
            disabled={!canMutate}
            onChange={(e) => {
              setShiftPatternCode(e.target.value);
              markPersonalized();
            }}
            className="h-9"
          />
        </div>
        <div className="flex items-end gap-2 pb-1">
          <Checkbox
            id="is-continuous"
            checked={isContinuous}
            disabled={!canMutate}
            onCheckedChange={(v) => {
              setIsContinuous(v === true);
              markPersonalized();
            }}
          />
          <Label htmlFor="is-continuous">{t("schedulePattern.isContinuous")}</Label>
        </div>
      </div>

      <div className="space-y-1.5">
        <p className="text-xs font-medium text-text-muted">
          {t("schedulePattern.templates.title")}
        </p>
        <div
          className="flex flex-wrap gap-1.5"
          role="group"
          aria-label={t("schedulePattern.templates.title")}
        >
          {SCHEDULE_TEMPLATE_IDS.map((id) => (
            <Button
              key={id}
              type="button"
              size="sm"
              variant={activeTemplate === id ? "default" : "outline"}
              className="h-7 text-xs"
              disabled={!canMutate && id !== "personalized"}
              onClick={() => handleApplyTemplate(id)}
            >
              {t(`schedulePattern.templates.${id}`)}
            </Button>
          ))}
        </div>
      </div>

      <div className={cn(mfCard.insetCanvas, "min-h-0 space-y-1 p-3")}>
        <p className="mb-2 text-xs font-medium text-text-muted">
          {t("schedulePattern.weekCanvas")}
        </p>
        <div
          role="listbox"
          aria-label={t("schedulePattern.weekCanvas")}
          aria-activedescendant={`schedule-day-${selectedDay}`}
          tabIndex={0}
          onKeyDown={onWeekKeyDown}
          className="space-y-1 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/40"
        >
          {days.map((day) => {
            const isSelected = day.dayOfWeek === selectedDay;
            const dayName = t(`schedulePattern.days.${DAY_KEYS[day.dayOfWeek - 1]}`);
            const interval = day.intervals[0];
            const segments =
              day.mode === "work" && interval
                ? intervalBarSegments(interval.start, interval.end)
                : [];
            const hours =
              day.mode === "work" ? formatHours(dayDurationHours(day)) : null;

            return (
              <div
                key={day.dayOfWeek}
                id={`schedule-day-${day.dayOfWeek}`}
                role="option"
                aria-selected={isSelected}
                aria-label={
                  day.mode === "rest"
                    ? t("schedulePattern.dayAriaRest", { day: dayName })
                    : t("schedulePattern.dayAriaWork", {
                        day: dayName,
                        start: interval?.start ?? "",
                        end: interval?.end ?? "",
                      })
                }
                className={cn(
                  "flex w-full flex-wrap items-center gap-2 rounded-md border px-2 py-1.5 text-left transition-colors",
                  "focus-within:ring-2 focus-within:ring-primary/40",
                  isSelected
                    ? "border-primary/50 bg-primary/5"
                    : "cursor-pointer border-transparent hover:bg-surface-1",
                  day.mode === "rest" && "bg-surface-2/60",
                )}
                onClick={() => setSelectedDay(day.dayOfWeek)}
              >
                <span className="w-16 shrink-0 text-xs font-medium text-text-primary">
                  {t(`schedulePattern.daysShort.${DAY_KEYS[day.dayOfWeek - 1]}`)}
                </span>
                <div className="relative h-6 min-w-0 flex-1 rounded bg-surface-2/80">
                  {day.mode === "rest" ? (
                    <div className="flex h-full items-center justify-center text-[10px] text-text-muted">
                      {t("schedulePattern.restDayLabel")}
                    </div>
                  ) : (
                    segments.map((seg, i) => (
                      <div
                        key={i}
                        className="absolute top-0.5 bottom-0.5 rounded-sm bg-primary"
                        style={{ left: `${seg.leftPct}%`, width: `${seg.widthPct}%` }}
                      />
                    ))
                  )}
                </div>

                {isSelected ? (
                  <div
                    className="flex shrink-0 flex-wrap items-center gap-2"
                    onClick={(e) => e.stopPropagation()}
                    onKeyDown={(e) => e.stopPropagation()}
                  >
                    <label className="flex items-center gap-1.5 text-xs text-text-primary">
                      <input
                        type="checkbox"
                        checked={day.mode === "rest"}
                        disabled={!canMutate}
                        aria-label={t("schedulePattern.mode.rest")}
                        className="h-3.5 w-3.5 accent-primary"
                        onChange={(e) =>
                          patchDay(updateDayMode(day, e.target.checked ? "rest" : "work"))
                        }
                      />
                      {t("schedulePattern.restDay")}
                    </label>
                    {day.mode === "work" ? (
                      <>
                        <TimeInput
                          id={`schedule-start-${day.dayOfWeek}`}
                          value={interval?.start ?? ""}
                          disabled={!canMutate}
                          aria-label={t("schedulePattern.start")}
                          className="h-8 w-[7.5rem]"
                          onChange={(v) =>
                            patchDay(updateDayInterval(day, { start: v }))
                          }
                        />
                        <span className="text-xs text-text-muted">–</span>
                        <TimeInput
                          id={`schedule-end-${day.dayOfWeek}`}
                          value={interval?.end ?? ""}
                          disabled={!canMutate}
                          aria-label={t("schedulePattern.end")}
                          className="h-8 w-[7.5rem]"
                          onChange={(v) =>
                            patchDay(updateDayInterval(day, { end: v }))
                          }
                        />
                      </>
                    ) : (
                      <span className="text-xs text-text-muted">
                        {t("schedulePattern.restDayLabel")}
                      </span>
                    )}
                  </div>
                ) : (
                  <span className="w-20 shrink-0 text-right font-mono text-[10px] tabular-nums text-text-muted">
                    {day.mode === "rest"
                      ? "—"
                      : `${interval?.start ?? ""}–${interval?.end ?? ""}`}
                  </span>
                )}

                <span className="w-10 shrink-0 text-right text-[10px] text-text-muted">
                  {hours != null ? `${hours}h` : ""}
                </span>
              </div>
            );
          })}
        </div>
      </div>

      {!validation.valid && validation.errorKey ? (
        <p className="text-xs text-status-danger" role="alert">
          {t(`schedulePattern.validation.${validation.errorKey}`)}
        </p>
      ) : null}
      {error ? (
        <p className="text-xs text-status-danger" role="alert">
          {error}
        </p>
      ) : null}
      {!canMutate ? (
        <p className="text-xs text-text-muted">{t("schedulePattern.readOnly")}</p>
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

/** Centered wide entity dialog hosting the weekly calendar editor. */
export function SchedulePatternDialog({
  value,
  open,
  onOpenChange,
  canMutate,
  onSaved,
}: SchedulePatternDialogProps) {
  const { t } = useTranslation("reference");
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
      title={t("schedulePattern.title")}
      description={`${value.code} — ${value.label}`}
      footer={
        <EntityFormFooter
          cancelLabel={t("schedulePattern.cancel")}
          primaryLabel={
            chrome.saving ? t("schedulePattern.saving") : t("schedulePattern.save")
          }
          onCancel={() => onOpenChange(false)}
          onPrimary={() => chrome.save()}
          primaryType="button"
          cancelDisabled={chrome.saving}
          primaryDisabled={!chrome.canSave}
          primaryLoading={chrome.saving}
        />
      }
    >
      <SchedulePatternEditor
        key={value.id}
        value={value}
        canMutate={canMutate}
        onRequestClose={() => onOpenChange(false)}
        onChromeChange={handleChromeChange}
        {...(onSaved ? { onSaved } : {})}
      />
    </EntityFormDialog>
  );
}

/** @deprecated Prefer SchedulePatternDialog — kept for import stability. */
export const SchedulePatternPanel = SchedulePatternDialog;

function Metric({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <p className="text-[10px] uppercase tracking-wide text-text-muted">{label}</p>
      <p className="text-sm font-semibold text-text-primary">{value}</p>
    </div>
  );
}

function formatHours(h: number): string {
  return Number.isInteger(h) ? String(h) : h.toFixed(1);
}
