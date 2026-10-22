import type { TFunction } from "i18next";
import { useTranslation } from "react-i18next";

import { useFormatters } from "@/hooks/use-formatters";
import type { AssigneeCapacity } from "@shared/ipc-types";

type AssigneeCapacityDetailsProps = {
  /** The chosen technician over the slot (`list_assignee_capacity`), if loaded. */
  capacity: AssigneeCapacity | undefined;
  /** The job's planned crew size: the others are counted in the crew's load, not named. */
  crewSize?: number | null;
  /** A technician is named on the job. */
  assigned: boolean;
};

/**
 * The chosen technician's days over the slot (hours left and shift each day, the night before when
 * its shift runs into the slot), what a commit would refuse, and what the figures leave out.
 */
export function AssigneeCapacityDetails({
  capacity,
  crewSize,
  assigned,
}: AssigneeCapacityDetailsProps) {
  const { t, i18n } = useTranslation("planning");
  const { formatDecimal } = useFormatters();
  const others = crewSize != null && crewSize > 1 ? crewSize - (assigned ? 1 : 0) : 0;
  return (
    <div className="mt-1 space-y-1">
      {capacity && capacity.days.length > 0 ? (
        <ul className="flex flex-wrap gap-1" aria-label={t("assignee.daysLabel")}>
          {capacity.days.map((d) => (
            <li
              key={d.day}
              className="rounded border border-surface-border bg-surface-1 px-1.5 py-0.5 text-[10px] text-text-secondary"
            >
              <span className="font-medium text-text-primary">
                {dayLabel(d.day, i18n.language)}
              </span>
              {" · "}
              {d.shift_start && d.shift_end
                ? `${d.shift_start}–${d.shift_end}`
                : t(`assignee.reasons.${d.reason}`, { defaultValue: d.reason })}
              {" · "}
              {t("assignee.hoursLeft", { hours: formatDecimal(d.remaining_hours, 1) })}
            </li>
          ))}
        </ul>
      ) : null}
      {capacity && capacity.issues.length > 0 ? (
        <ul className="space-y-0.5 text-[11px] text-tone-attention-fg" role="status">
          {capacity.issues.map((code) => (
            <li key={code}>{t(`assignee.issues.${code}`)}</li>
          ))}
        </ul>
      ) : null}
      {capacity && capacity.unknown_hours_jobs > 0 ? (
        <p className="text-[11px] text-text-muted">
          {t("assignee.unknownHours", { count: capacity.unknown_hours_jobs })}
        </p>
      ) : null}
      {capacity && capacity.unreadable_work_orders.length > 0 ? (
        <p className="text-[11px] text-tone-attention-fg">
          {t("assignee.unreadableWorkOrders", {
            codes: capacity.unreadable_work_orders.map((w) => w.code).join(", "),
          })}
        </p>
      ) : null}
      {others > 0 ? (
        <p className="text-[11px] text-text-muted">
          {t("assignee.othersUnnamed", { count: others })}
        </p>
      ) : null}
    </div>
  );
}

function dayLabel(day: string, locale: string): string {
  const date = new Date(`${day}T12:00:00`);
  return Number.isNaN(date.getTime())
    ? day
    : date.toLocaleDateString(locale, { weekday: "short", day: "2-digit", month: "2-digit" });
}

/** The picker line: hours left each day ("2.0 / 8.0 h left · 08:00–16:00"), or why not working. */
export function assigneeOptionSuffix(
  cap: AssigneeCapacity,
  t: TFunction<"planning">,
  formatDecimal: (value: number, decimals?: number) => string,
): string {
  if (cap.issues.includes("PLANNING_ASSIGNEE_NOT_WORKING") || cap.slot_working_hours <= 0) {
    const off = cap.days.find((d) => d.reason !== "working");
    return t("assignee.notWorking", {
      reason: t(`assignee.reasons.${off?.reason ?? "outside_shift"}`),
    });
  }
  const hours = t("assignee.hoursLeft", {
    hours: cap.days.map((d) => formatDecimal(d.remaining_hours, 1)).join(" / "),
  });
  const shift = cap.days.find((d) => d.shift_start && d.shift_end);
  return shift ? `${hours} · ${shift.shift_start}–${shift.shift_end}` : hours;
}
