/**
 * The shift a work order is planned on: one of the windows its team works at the planned start,
 * from the members' Classes horaires (`list_wo_shift_options`); an early-morning start also offers
 * the previous night's shift still running. Nothing is offered without a team, a date, or anybody
 * working then — the shift then stays empty, never assumed. The stored shift is shown as planned;
 * the parent sends it only when the planner changes it (`wo-shift-plan`).
 */
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Link } from "react-router-dom";

import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useFormatters } from "@/hooks/use-formatters";
import { type WoShiftOptionsResult, listWoShiftOptions } from "@/services/wo-service";
import type { WorkOrder } from "@shared/ipc-types";

import { plannedOutsideWindow } from "./wo-shift-plan";

const NONE = "__none";

export interface WoShiftSelectProps {
  wo: WorkOrder;
  /** Planned start / end as the form holds them (local `YYYY-MM-DDTHH:mm`, or the stored instant). */
  plannedStart: string | null;
  plannedEnd?: string | null;
  /** The team chosen in the assignment section (may not be saved yet); default: assigned team. */
  teamId?: number | null;
  /** Chosen `class_version_id` ("" = no shift). */
  value: string;
  onChange: (value: string) => void;
  disabled?: boolean;
}

function formatWindow(start: string | null, end: string | null): string {
  return start && end ? `${start}–${end}` : "";
}

export function WoShiftSelect({
  wo,
  plannedStart,
  plannedEnd = null,
  teamId = null,
  value,
  onChange,
  disabled,
}: WoShiftSelectProps) {
  const { t } = useTranslation("ot");
  const { formatNumber } = useFormatters();
  const [result, setResult] = useState<WoShiftOptionsResult | null>(null);
  const [failed, setFailed] = useState(false);
  // A team being assigned: its shifts are offered, saved with the assignment.
  const newTeam = teamId != null && teamId !== wo.assigned_team_id;

  useEffect(() => {
    let cancelled = false;
    // Never show the options of another team or day while the new ones load.
    setResult(null);
    setFailed(false);
    listWoShiftOptions(wo.id, plannedStart, teamId)
      .then((r) => {
        if (!cancelled) setResult(r);
      })
      .catch(() => {
        if (!cancelled) setFailed(true);
      });
    return () => {
      cancelled = true;
    };
  }, [wo.id, wo.assigned_team_id, plannedStart, teamId]);

  const options = result?.options ?? [];
  const selected = options.find((o) => String(o.class_version_id) === value) ?? null;
  // The shift stored on the work order, when the team does not work it then: kept as planned.
  const currentNotOffered =
    value !== "" &&
    selected == null &&
    wo.shift_class_version_id != null &&
    String(wo.shift_class_version_id) === value;
  const chosenWindow =
    selected ??
    (currentNotOffered && wo.shift_start && wo.shift_end
      ? { shift_start: wo.shift_start, shift_end: wo.shift_end }
      : null);
  const outside =
    chosenWindow != null && plannedOutsideWindow(plannedStart, plannedEnd, chosenWindow);
  // The work order keeps the window it was planned with; its version was edited since.
  const timetableChanged =
    selected != null &&
    wo.shift_class_version_id != null &&
    String(wo.shift_class_version_id) === value &&
    wo.shift_start != null &&
    (selected.shift_start !== wo.shift_start || selected.shift_end !== wo.shift_end);

  return (
    <div className="space-y-1">
      <Label>{t("planning.shift")}</Label>
      <Select
        value={value || NONE}
        onValueChange={(v) => onChange(v === NONE ? "" : v)}
        disabled={disabled ?? false}
      >
        <SelectTrigger>
          <SelectValue placeholder={t("planning.selectShift")} />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value={NONE}>{t("planning.shiftNone")}</SelectItem>
          {options.map((o) => (
            <SelectItem key={o.class_version_id} value={String(o.class_version_id)}>
              {t("planning.shiftOption", {
                label: o.class_label,
                window: formatWindow(o.shift_start, o.shift_end),
                hours: formatNumber(o.net_hours),
                members: o.members,
              })}
              {o.starts_previous_day ? ` · ${t("planning.shiftPreviousNight")}` : ""}
            </SelectItem>
          ))}
          {currentNotOffered ? (
            <SelectItem value={value} disabled>
              {t("planning.shiftCurrentNotOffered", {
                label: wo.shift_class_label ?? "",
                window: formatWindow(wo.shift_start, wo.shift_end),
              })}
            </SelectItem>
          ) : null}
        </SelectContent>
      </Select>
      {timetableChanged ? (
        <p className="text-xs text-text-muted" role="status">
          {t("planning.shiftTimetableChanged", {
            planned: formatWindow(wo.shift_start, wo.shift_end),
            now: formatWindow(selected.shift_start, selected.shift_end),
          })}
        </p>
      ) : null}
      {outside ? (
        <p className="text-xs text-text-muted" role="status">
          {t("planning.shiftOutsideWindow")}
        </p>
      ) : null}
      {newTeam ? <p className="text-xs text-text-muted">{t("planning.shiftForNewTeam")}</p> : null}
      {wo.shift && !wo.shift_class_version_id ? (
        <p className="text-xs text-text-muted">
          {t("planning.shiftLegacy", { label: t(`shift.${wo.shift}`) })}
        </p>
      ) : null}
      {failed ? <p className="text-xs text-text-muted">{t("planning.shiftLoadFailed")}</p> : null}
      {result?.reason === "no_team" ? (
        <p className="text-xs text-text-muted">{t("planning.shiftNoTeam")}</p>
      ) : null}
      {result?.reason === "no_day" ? (
        <p className="text-xs text-text-muted">{t("planning.shiftNoDay")}</p>
      ) : null}
      {result?.reason === "no_shift" ? (
        <p className="text-xs text-text-muted">
          {t("planning.shiftNoShift")}{" "}
          <Link className="underline" to={`/personnel?tab=teams&team=${result.team_id ?? ""}`}>
            {t("planning.shiftOpenTeam")}
          </Link>
        </p>
      ) : null}
    </div>
  );
}
