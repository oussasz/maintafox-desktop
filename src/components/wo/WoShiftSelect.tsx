/**
 * The shift a work order is planned on: one of the windows its assigned team works on the planned
 * day, from the members' Classes horaires (`list_wo_shift_options`). Nothing is offered without a
 * team, a date, or anybody working that day — the shift then stays empty, never assumed.
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
import { type WoShiftOptionsResult, listWoShiftOptions } from "@/services/wo-service";
import type { WorkOrder } from "@shared/ipc-types";

const NONE = "__none";

export interface WoShiftSelectProps {
  wo: WorkOrder;
  /** Day the work order will be planned on (`YYYY-MM-DD`), as the save sends it. */
  plannedDay: string | null;
  /** Chosen `class_version_id` ("" = no shift). */
  value: string;
  onChange: (value: string) => void;
  disabled?: boolean;
}

function formatWindow(start: string | null, end: string | null): string {
  return start && end ? `${start}–${end}` : "";
}

export function WoShiftSelect({ wo, plannedDay, value, onChange, disabled }: WoShiftSelectProps) {
  const { t } = useTranslation("ot");
  const [result, setResult] = useState<WoShiftOptionsResult | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    let cancelled = false;
    setFailed(false);
    listWoShiftOptions(wo.id, plannedDay)
      .then((r) => {
        if (!cancelled) setResult(r);
      })
      .catch(() => {
        if (!cancelled) {
          setResult(null);
          setFailed(true);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [wo.id, wo.assigned_team_id, plannedDay]);

  const options = result?.options ?? [];
  const offered = options.some((o) => String(o.class_version_id) === value);
  // The work order's current window, when the team no longer works it that day.
  const currentNotOffered =
    value !== "" &&
    !offered &&
    wo.shift_class_version_id != null &&
    String(wo.shift_class_version_id) === value;

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
                hours: o.net_hours.toLocaleString(undefined, { maximumFractionDigits: 2 }),
                members: o.members,
              })}
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
