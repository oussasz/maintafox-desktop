/**
 * A team's Classe horaire: the one in force today (with its utilisation factor), the history of
 * changes, and a change from a date (today or later — past capacity and plans are never
 * rewritten). Members inherit it unless they have their own Classe horaire or a dated exception.
 * A link created by the upgrade is flagged for review until a planner confirms or replaces it.
 */
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { FormField } from "@/components/ui/FormField";
import { Button } from "@/components/ui/button";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { StatusChip } from "@/components/ui/status-chip";
import { mfTone } from "@/design-system/tone";
import { usePermissions } from "@/hooks/use-permissions";
import { cn } from "@/lib/utils";
import {
  type ScheduleClassVersion,
  type TeamScheduleAssignment,
  confirmTeamScheduleReview,
  listScheduleClassVersions,
  listTeamSchedule,
  setTeamSchedule,
} from "@/services/workforce-service";
import { formatOrgIpcError } from "@/utils/errors";
import { P } from "@shared/rbac/permissions.generated";

const NONE = "__none__";

function todayIso(): string {
  const d = new Date();
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

/** Latest row starting on or before `day` (rows sorted oldest first). */
function inForceOn<T extends { effective_from: string }>(rows: T[], day: string): T | undefined {
  return [...rows].reverse().find((r) => r.effective_from <= day);
}

export function TeamTimetablePanel({
  teamId,
  onChanged,
}: {
  teamId: number;
  /** Capacity shown elsewhere on the page depends on the link. */
  onChanged?: () => void;
}) {
  const { t, i18n } = useTranslation("personnel");
  const { can } = usePermissions();
  const canManage = can(P.PER_MANAGE);
  const today = todayIso();
  const [links, setLinks] = useState<TeamScheduleAssignment[]>([]);
  const [versions, setVersions] = useState<ScheduleClassVersion[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<unknown>(null);
  const [choice, setChoice] = useState<string>(NONE);
  const [from, setFrom] = useState(today);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);

  const dateFmt = useMemo(
    () => new Intl.DateTimeFormat(i18n.language, { dateStyle: "medium", timeZone: "UTC" }),
    [i18n.language],
  );
  const num = useMemo(
    () => new Intl.NumberFormat(i18n.language, { maximumFractionDigits: 1 }),
    [i18n.language],
  );

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [l, v] = await Promise.all([listTeamSchedule(teamId), listScheduleClassVersions(null)]);
      setLinks(l);
      setVersions(v);
      const current = inForceOn(l, today);
      setChoice(
        current?.schedule_reference_value_id != null
          ? String(current.schedule_reference_value_id)
          : NONE,
      );
    } catch (e) {
      setError(e);
    } finally {
      setLoading(false);
    }
  }, [teamId, today]);

  useEffect(() => {
    void load();
  }, [load]);

  /** Classes horaires that have a version in force today (usable for capacity now). */
  const classes = useMemo(() => {
    const byClass = new Map<number, ScheduleClassVersion[]>();
    for (const v of versions) {
      const list = byClass.get(v.reference_value_id) ?? [];
      list.push(v);
      byClass.set(v.reference_value_id, list);
    }
    return [...byClass.entries()]
      .map(([id, list]) => ({ id, version: inForceOn(list, today) ?? list[0] }))
      .filter((c): c is { id: number; version: ScheduleClassVersion } => c.version != null)
      .sort((a, b) => a.version.class_label.localeCompare(b.version.class_label));
  }, [today, versions]);

  const current = inForceOn(links, today);
  const currentClass = classes.find((c) => c.id === current?.schedule_reference_value_id);
  const toReview = links.some((l) => l.review_status === "to_review");

  const apply = async () => {
    setSaving(true);
    setSaveError(null);
    try {
      setLinks(
        await setTeamSchedule({
          team_id: teamId,
          schedule_reference_value_id: choice === NONE ? null : Number(choice),
          effective_from: from,
        }),
      );
      onChanged?.();
    } catch (e) {
      setSaveError(formatOrgIpcError(e));
    } finally {
      setSaving(false);
    }
  };

  const confirm = async () => {
    setSaving(true);
    setSaveError(null);
    try {
      setLinks(await confirmTeamScheduleReview(teamId));
    } catch (e) {
      setSaveError(formatOrgIpcError(e));
    } finally {
      setSaving(false);
    }
  };

  if (error && links.length === 0) return <ErrorState error={error} onRetry={() => void load()} />;
  if (loading && links.length === 0 && versions.length === 0) return <LoadingState />;

  return (
    <div className="space-y-3">
      {toReview ? (
        <div role="status" className={cn("rounded-md border p-3 text-sm", mfTone.attention.panel)}>
          <p className="font-medium text-text-primary">{t("timetable.review.teamTitle")}</p>
          <p className="mt-1 text-text-secondary">
            {t("timetable.review.teamBody", { name: current?.schedule_label ?? "—" })}
          </p>
          {canManage ? (
            <Button
              size="sm"
              variant="outline"
              className="mt-2"
              disabled={saving}
              onClick={() => void confirm()}
            >
              {t("timetable.review.confirm")}
            </Button>
          ) : null}
        </div>
      ) : null}
      {!current?.schedule_reference_value_id ? (
        <div role="alert" className={cn("rounded-md border p-3 text-sm", mfTone.warning.panel)}>
          <p className="font-medium text-text-primary">{t("timetable.team.noneTitle")}</p>
          <p className="mt-1 text-text-secondary">{t("timetable.team.noneBody")}</p>
        </div>
      ) : (
        <dl className="grid grid-cols-2 gap-2 text-sm sm:grid-cols-4">
          <div>
            <dt className="text-xs text-text-muted">{t("timetable.team.current")}</dt>
            <dd className="font-semibold">{current.schedule_label}</dd>
          </div>
          <div>
            <dt className="text-xs text-text-muted">{t("timetable.editor.cycle")}</dt>
            <dd>
              {currentClass?.version.rotation_start_date
                ? t("timetable.editor.rotationOf", { days: currentClass.version.cycle_length_days })
                : t("timetable.editor.weekly")}
            </dd>
          </div>
          <div>
            <dt className="text-xs text-text-muted">{t("timetable.editor.netHours")}</dt>
            <dd className="tabular-nums">
              {currentClass
                ? t("timetable.editor.hoursPerCycle", {
                    hours: num.format(currentClass.version.net_hours_per_cycle),
                    days: currentClass.version.working_days_per_cycle,
                  })
                : t("timetable.team.notSetUp")}
            </dd>
          </div>
          <div>
            <dt className="text-xs text-text-muted">{t("timetable.editor.utilisation")}</dt>
            <dd className="tabular-nums">
              {currentClass ? `${num.format(currentClass.version.utilisation_percent)} %` : "—"}
            </dd>
          </div>
        </dl>
      )}
      <p className="text-xs text-text-muted">{t("timetable.team.inheritHint")}</p>

      {canManage ? (
        <div className="flex flex-wrap items-end gap-3 rounded-md border border-surface-border p-3">
          <FormField name="team-class" label={t("timetable.team.choose")} className="min-w-[16rem]">
            <Select value={choice} onValueChange={setChoice}>
              <SelectTrigger id="team-class">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={NONE}>{t("timetable.team.none")}</SelectItem>
                {classes.map((c) => (
                  <SelectItem key={c.id} value={String(c.id)}>
                    {t("timetable.team.option", {
                      name: c.version.class_label,
                      utilisation: num.format(c.version.utilisation_percent),
                    })}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </FormField>
          <FormField name="team-class-from" label={t("timetable.editor.effectiveFrom")}>
            <Input
              id="team-class-from"
              type="date"
              min={today}
              value={from}
              onChange={(e) => setFrom(e.target.value)}
              className="h-9"
            />
          </FormField>
          <Button disabled={saving || from < today} onClick={() => void apply()}>
            {t("timetable.team.apply")}
          </Button>
          {classes.length === 0 ? (
            <p className="basis-full text-xs text-text-muted">{t("timetable.team.noClasses")}</p>
          ) : null}
        </div>
      ) : null}
      {saveError ? (
        <p className={cn("text-xs", mfTone.danger.text)} role="alert">
          {saveError}
        </p>
      ) : null}

      {links.length > 0 ? (
        <table className="w-full text-sm">
          <caption className="pb-1 text-left text-xs font-medium text-text-muted">
            {t("timetable.team.history")}
          </caption>
          <thead>
            <tr className="border-b border-surface-border text-left text-xs text-text-muted">
              <th className="py-1.5 pr-2 font-medium">{t("timetable.editor.from")}</th>
              <th className="py-1.5 pr-2 font-medium">{t("timetable.team.class")}</th>
              <th className="py-1.5 font-medium">{t("timetable.editor.status")}</th>
            </tr>
          </thead>
          <tbody>
            {links.map((l) => (
              <tr key={l.id} className="border-b border-surface-border/60">
                <td className="py-1.5 pr-2">
                  {dateFmt.format(new Date(`${l.effective_from}T00:00:00Z`))}
                </td>
                <td className="py-1.5 pr-2">{l.schedule_label ?? t("timetable.team.none")}</td>
                <td className="py-1.5">
                  <span className="flex flex-wrap gap-1">
                    <StatusChip
                      label={t(
                        current?.id === l.id
                          ? "timetable.editor.inForce"
                          : l.effective_from > today
                            ? "timetable.editor.planned"
                            : "timetable.editor.past",
                      )}
                      tone={
                        current?.id === l.id
                          ? "success"
                          : l.effective_from > today
                            ? "info"
                            : "neutral"
                      }
                      dot
                    />
                    {l.review_status === "to_review" ? (
                      <StatusChip label={t("timetable.review.chip")} tone="attention" dot />
                    ) : null}
                  </span>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      ) : null}
    </div>
  );
}
