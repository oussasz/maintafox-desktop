/**
 * Real capacity day by day — from the Classes horaires on the site's working days, minus
 * absences, × utilisation (`personnel::capacity`). Team view: hours available for work orders,
 * hours at work, people on shift / absent, the Classe horaire of the day and the planner's
 * overtime ceiling. Person view: Classe horaire, shift, hours and why.
 */
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { usePersonnelLists } from "@/components/personnel/use-personnel-lists";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { Input } from "@/components/ui/input";
import { StatusChip } from "@/components/ui/status-chip";
import { mfTone, toneOf } from "@/design-system/tone";
import { cn } from "@/lib/utils";
import {
  CAPACITY_MAX_RANGE_DAYS,
  type PersonDayCapacity,
  type TeamDayCapacity,
  getPersonCapacity,
  getTeamCapacity,
} from "@/services/workforce-service";

import {
  CAPACITY_BASIS_TONE,
  DAY_REASON_TONE,
  todayIso,
} from "./qualification/qualification-tones";

/** Days shown by default (two working weeks). */
export const DEFAULT_CAPACITY_DAYS = 14;

function plusDays(iso: string, days: number): string {
  const d = new Date(`${iso}T00:00:00Z`);
  d.setUTCDate(d.getUTCDate() + days);
  return d.toISOString().slice(0, 10);
}

function useDayFormatter() {
  const { i18n } = useTranslation();
  return useMemo(
    () =>
      new Intl.DateTimeFormat(i18n.language, {
        weekday: "short",
        day: "2-digit",
        month: "2-digit",
        timeZone: "UTC",
      }),
    [i18n.language],
  );
}

function hours(n: number, locale: string): string {
  return new Intl.NumberFormat(locale, { maximumFractionDigits: 1 }).format(n);
}

function RangePicker({
  from,
  to,
  onChange,
}: {
  from: string;
  to: string;
  onChange: (from: string, to: string) => void;
}) {
  const { t } = useTranslation("personnel");
  const tooLong =
    (new Date(to).getTime() - new Date(from).getTime()) / 86_400_000 >= CAPACITY_MAX_RANGE_DAYS;
  return (
    <div className="flex flex-wrap items-end gap-2 text-sm">
      <label className="space-y-1">
        <span className="block text-text-muted">{t("capacityDays.from")}</span>
        <Input
          type="date"
          className="h-9 w-40"
          value={from}
          onChange={(e) => onChange(e.target.value, to)}
        />
      </label>
      <label className="space-y-1">
        <span className="block text-text-muted">{t("capacityDays.to")}</span>
        <Input
          type="date"
          className="h-9 w-40"
          value={to}
          min={from}
          onChange={(e) => onChange(from, e.target.value)}
        />
      </label>
      {tooLong || to < from ? (
        <p className={cn("pb-2", mfTone.danger.text)} role="alert">
          {t("capacityDays.rangeInvalid", { max: CAPACITY_MAX_RANGE_DAYS })}
        </p>
      ) : null}
    </div>
  );
}

export function TeamCapacityDays({ teamId }: { teamId: number }) {
  const { t, i18n } = useTranslation("personnel");
  const dayFmt = useDayFormatter();
  const [from, setFrom] = useState(todayIso());
  const [to, setTo] = useState(plusDays(todayIso(), DEFAULT_CAPACITY_DAYS - 1));
  const [days, setDays] = useState<TeamDayCapacity[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<unknown>(null);
  const [expanded, setExpanded] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (to < from) return;
    setLoading(true);
    setError(null);
    try {
      setDays(await getTeamCapacity(teamId, from, to));
    } catch (err) {
      setError(err);
    } finally {
      setLoading(false);
    }
  }, [teamId, from, to]);

  useEffect(() => {
    void load();
  }, [load]);

  const total = days.reduce((s, d) => s + d.available_hours, 0);

  return (
    <div className="space-y-3">
      <RangePicker
        from={from}
        to={to}
        onChange={(f, tt) => {
          setFrom(f);
          setTo(tt);
        }}
      />
      <p className="text-sm text-text-muted">{t("capacityDays.teamIntro")}</p>
      {error && days.length === 0 ? (
        <ErrorState error={error} onRetry={() => void load()} retrying={loading} />
      ) : loading && days.length === 0 ? (
        <LoadingState />
      ) : (
        <>
          <p className="text-sm font-medium text-text-primary">
            {t("capacityDays.totalHours", { hours: hours(total, i18n.language) })}
          </p>
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-surface-border text-left text-xs text-text-muted">
                <th className="py-1.5 pr-2 font-medium">{t("capacityDays.day")}</th>
                <th className="py-1.5 pr-2 font-medium">{t("capacityDays.classe")}</th>
                <th className="py-1.5 pr-2 text-right font-medium">
                  {t("capacityDays.available")}
                </th>
                <th className="py-1.5 pr-2 text-right font-medium">
                  {t("capacityDays.attendance")}
                </th>
                <th className="py-1.5 pr-2 text-right font-medium">
                  {t("capacityDays.absentHours")}
                </th>
                <th className="py-1.5 pr-2 text-right font-medium">{t("capacityDays.onShift")}</th>
                <th className="py-1.5 pr-2 font-medium">{t("capacityDays.basis")}</th>
                <th className="py-1.5 text-right font-medium">{t("capacityDays.overtime")}</th>
              </tr>
            </thead>
            <tbody>
              {days.map((d) => (
                <FragmentRow
                  key={d.day}
                  day={d}
                  label={dayFmt.format(new Date(`${d.day}T00:00:00Z`))}
                  expanded={expanded === d.day}
                  onToggle={() => setExpanded(expanded === d.day ? null : d.day)}
                />
              ))}
            </tbody>
          </table>
        </>
      )}
    </div>
  );
}

function FragmentRow({
  day,
  label,
  expanded,
  onToggle,
}: {
  day: TeamDayCapacity;
  label: string;
  expanded: boolean;
  onToggle: () => void;
}) {
  const { t, i18n } = useTranslation("personnel");
  return (
    <>
      <tr className="border-b border-surface-border/60">
        <td className="py-1.5 pr-2">
          <button
            type="button"
            className="text-left hover:underline focus:outline-none focus-visible:ring-2 focus-visible:ring-ring"
            aria-expanded={expanded}
            onClick={onToggle}
          >
            {label}
          </button>
        </td>
        <td className="py-1.5 pr-2 text-text-secondary">{day.schedule_class_label ?? "—"}</td>
        <td className="py-1.5 pr-2 text-right font-medium tabular-nums">
          {hours(day.available_hours, i18n.language)} h
        </td>
        <td className="py-1.5 pr-2 text-right tabular-nums text-text-muted">
          {hours(day.attendance_hours, i18n.language)} h
        </td>
        <td className="py-1.5 pr-2 text-right tabular-nums text-text-muted">
          {hours(day.absent_hours, i18n.language)} h
        </td>
        <td className="py-1.5 pr-2 text-right tabular-nums">
          {day.people_scheduled - day.people_absent}/{day.people}
        </td>
        <td className="py-1.5 pr-2">
          <StatusChip
            label={t(`capacityDays.bases.${day.basis}`)}
            tone={toneOf(CAPACITY_BASIS_TONE, day.basis)}
            dot
          />
        </td>
        <td className="py-1.5 text-right tabular-nums text-text-muted">
          {day.rule_overtime_hours != null
            ? `${hours(day.rule_overtime_hours, i18n.language)} h`
            : "—"}
        </td>
      </tr>
      {expanded ? (
        <tr>
          <td colSpan={8} className="bg-surface-1 px-3 py-2">
            {day.members.length === 0 ? (
              <p className="text-sm text-text-muted">{t("capacityDays.noMembers")}</p>
            ) : (
              <ul className="grid gap-1 sm:grid-cols-2">
                {day.members.map((m) => (
                  <li
                    key={m.personnel_id}
                    className="flex items-center justify-between gap-2 text-sm"
                  >
                    <span>
                      {m.full_name}
                      {m.allocation_percent < 100 ? (
                        <span className="ml-1 text-xs text-text-muted">
                          ({m.allocation_percent} %)
                        </span>
                      ) : null}
                      {m.schedule_class_label ? (
                        <span className="ml-1 text-xs text-text-muted">
                          ·{" "}
                          {t(`capacityDays.sources.${m.schedule_source ?? "team"}`, {
                            name: m.schedule_class_label,
                          })}
                        </span>
                      ) : null}
                    </span>
                    <span className="flex items-center gap-2">
                      <span className="tabular-nums">
                        {hours(m.available_hours, i18n.language)} h
                      </span>
                      <StatusChip
                        label={t(`capacityDays.reasons.${m.reason}`)}
                        tone={toneOf(DAY_REASON_TONE, m.reason)}
                      />
                    </span>
                  </li>
                ))}
              </ul>
            )}
          </td>
        </tr>
      ) : null}
    </>
  );
}

export function PersonCapacityDays({ personnelId }: { personnelId: number }) {
  const { t, i18n } = useTranslation("personnel");
  // Absence types are named from the plant's list (Données de référence), by exact code.
  const lists = usePersonnelLists();
  const dayFmt = useDayFormatter();
  const from = todayIso();
  const to = plusDays(from, DEFAULT_CAPACITY_DAYS - 1);
  const [days, setDays] = useState<PersonDayCapacity[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<unknown>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setDays(await getPersonCapacity(personnelId, from, to));
    } catch (err) {
      setError(err);
    } finally {
      setLoading(false);
    }
  }, [personnelId, from, to]);

  useEffect(() => {
    void load();
  }, [load]);

  if (error && days.length === 0) return <ErrorState error={error} onRetry={() => void load()} />;
  if (loading && days.length === 0) return <LoadingState />;

  return (
    <div className="grid gap-2 sm:grid-cols-2">
      {days.map((d) => (
        <div
          key={d.day}
          className="flex items-center justify-between gap-2 rounded border border-surface-border px-2 py-1.5 text-sm"
        >
          <span>
            <span className="font-medium">{dayFmt.format(new Date(`${d.day}T00:00:00Z`))}</span>
            {d.schedule_class_label ? (
              <span className="ml-2 text-xs text-text-muted">
                {t(`capacityDays.sources.${d.schedule_source ?? "personal"}`, {
                  name: d.schedule_class_label,
                })}
              </span>
            ) : null}
            {d.shift_start && d.shift_end ? (
              <span className="ml-2 text-xs text-text-muted">
                {d.shift_start}–{d.shift_end}
                {d.break_minutes > 0
                  ? ` ${t("capacityDays.breakShort", { minutes: d.break_minutes })}`
                  : ""}
              </span>
            ) : null}
            {d.work_percent < 100 ? (
              <span className="ml-2 text-xs text-text-muted">
                {t("timetable.exceptions.partTime", { percent: d.work_percent })}
              </span>
            ) : null}
            {d.holiday_label ? (
              <span className="ml-2 text-xs text-text-muted">{d.holiday_label}</span>
            ) : null}
            {d.absence_types.length > 0 ? (
              <span className="ml-2 text-xs text-text-muted">
                {d.absence_types.map((a) => lists.text("absence_types", a)).join(", ")}
              </span>
            ) : null}
          </span>
          <span className="flex items-center gap-2">
            <span
              className="tabular-nums"
              title={t("capacityDays.personHoursTitle", {
                attendance: hours(d.available_minutes / 60, i18n.language),
                utilisation: d.utilisation_percent ?? "—",
              })}
            >
              {hours(d.plannable_minutes / 60, i18n.language)} h
            </span>
            <StatusChip
              label={t(`capacityDays.reasons.${d.reason}`)}
              tone={toneOf(DAY_REASON_TONE, d.reason)}
              dot
            />
          </span>
        </div>
      ))}
    </div>
  );
}
