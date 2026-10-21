/**
 * The crew's overtime ceiling per day (`capacity_rules.max_overtime_hours_per_day`): what planning
 * may add on top of the members' timetables. Each rule applies from a day, until a day or with no
 * end; periods of a crew never overlap (the backend refuses it). Planning reads it for the
 * capacity check, the board's utilisation and the week load. Reading needs the planning view
 * right, changing it the planning windows right — the same rights as the commands.
 */

import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { Button } from "@/components/ui/button";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useFormatters } from "@/hooks/use-formatters";
import { useToast } from "@/hooks/use-toast";
import { describeAppError } from "@/lib/app-error";
import {
  createCapacityRule,
  listCapacityRules,
  updateCapacityRule,
} from "@/services/planning-service";
import type { CapacityRule } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

function errorText(err: unknown): string {
  const { title, description } = describeAppError(err);
  return description ?? title;
}

/** Today as `YYYY-MM-DD` (local day). */
function today(): string {
  const d = new Date();
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

/** A number of hours typed by the planner: `null` when it is not a number of hours ≥ 0. */
export function parseHours(raw: string): number | null {
  const value = Number(raw.replace(",", "."));
  return raw.trim() !== "" && Number.isFinite(value) && value >= 0 ? value : null;
}

type Draft = { start: string; end: string; hours: string };

export function TeamOvertimeCeilingPanel({
  teamId,
  onChanged,
}: {
  teamId: number;
  /** A rule was created or changed: what reads the ceiling should reload. */
  onChanged?: () => void;
}) {
  const { t } = useTranslation("personnel");
  const { formatDate, formatNumber } = useFormatters();
  const { toast } = useToast();
  const [rules, setRules] = useState<CapacityRule[] | null>(null);
  const [loadError, setLoadError] = useState<unknown>(null);
  const [formError, setFormError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [draft, setDraft] = useState<Draft>({ start: today(), end: "", hours: "" });
  const [editing, setEditing] = useState<{ id: number; end: string; hours: string } | null>(null);

  const load = useCallback(async () => {
    setLoadError(null);
    try {
      const rows = await listCapacityRules({ team_id: teamId });
      setRules([...rows].sort((a, b) => b.effective_start.localeCompare(a.effective_start)));
    } catch (err) {
      setLoadError(err);
    }
  }, [teamId]);

  useEffect(() => {
    void load();
  }, [load]);

  async function run(action: () => Promise<unknown>, done: string) {
    setBusy(true);
    setFormError(null);
    try {
      await action();
      toast({ title: done, variant: "success" });
      await load();
      onChanged?.();
      return true;
    } catch (err) {
      setFormError(errorText(err));
      return false;
    } finally {
      setBusy(false);
    }
  }

  const draftHours = parseHours(draft.hours);
  const editHours = editing ? parseHours(editing.hours) : null;

  if (loadError != null) return <ErrorState error={loadError} onRetry={() => void load()} />;
  if (!rules) return <LoadingState />;

  return (
    <div className="space-y-3 text-sm">
      <p className="text-xs text-text-muted">{t("overtimeCeiling.hint")}</p>
      {rules.length === 0 ? (
        <p className="text-text-muted">{t("overtimeCeiling.none")}</p>
      ) : (
        <table className="w-full text-left">
          <thead>
            <tr className="text-xs text-text-muted">
              <th className="py-1 font-medium">{t("overtimeCeiling.from")}</th>
              <th className="py-1 font-medium">{t("overtimeCeiling.to")}</th>
              <th className="py-1 text-right font-medium">{t("overtimeCeiling.hoursPerDay")}</th>
              <th className="py-1" />
            </tr>
          </thead>
          <tbody>
            {rules.map((rule) =>
              editing?.id === rule.id ? (
                <tr key={rule.id} className="border-t border-surface-border">
                  <td className="py-1.5">{formatDate(rule.effective_start)}</td>
                  <td className="py-1.5">
                    <Input
                      type="date"
                      aria-label={t("overtimeCeiling.to")}
                      className="h-8 w-40"
                      min={rule.effective_start}
                      value={editing.end}
                      onChange={(e) => setEditing({ ...editing, end: e.target.value })}
                    />
                  </td>
                  <td className="py-1.5 text-right">
                    <Input
                      inputMode="decimal"
                      aria-label={t("overtimeCeiling.hoursPerDay")}
                      className="ml-auto h-8 w-24 text-right"
                      value={editing.hours}
                      onChange={(e) => setEditing({ ...editing, hours: e.target.value })}
                    />
                  </td>
                  <td className="py-1.5 text-right">
                    <div className="flex justify-end gap-1">
                      <Button
                        type="button"
                        size="sm"
                        disabled={busy || editHours == null}
                        onClick={() =>
                          void run(
                            () =>
                              updateCapacityRule(rule.id, rule.row_version, {
                                effective_end: editing.end || null,
                                clear_effective_end: editing.end === "",
                                max_overtime_hours_per_day: editHours ?? 0,
                              }),
                            t("overtimeCeiling.saved"),
                          ).then((ok) => ok && setEditing(null))
                        }
                      >
                        {t("overtimeCeiling.save")}
                      </Button>
                      <Button
                        type="button"
                        size="sm"
                        variant="outline"
                        disabled={busy}
                        onClick={() => setEditing(null)}
                      >
                        {t("overtimeCeiling.cancel")}
                      </Button>
                    </div>
                  </td>
                </tr>
              ) : (
                <tr key={rule.id} className="border-t border-surface-border">
                  <td className="py-1.5">{formatDate(rule.effective_start)}</td>
                  <td className="py-1.5">
                    {rule.effective_end
                      ? formatDate(rule.effective_end)
                      : t("overtimeCeiling.noEnd")}
                  </td>
                  <td className="py-1.5 text-right tabular-nums">
                    {t("overtimeCeiling.hours", {
                      hours: formatNumber(rule.max_overtime_hours_per_day, {
                        maximumFractionDigits: 2,
                      }),
                    })}
                  </td>
                  <td className="py-1.5 text-right">
                    <PermissionGate permission={P.PLAN_WINDOWS}>
                      <Button
                        type="button"
                        size="sm"
                        variant="outline"
                        disabled={busy}
                        onClick={() =>
                          setEditing({
                            id: rule.id,
                            end: rule.effective_end ?? "",
                            hours: String(rule.max_overtime_hours_per_day),
                          })
                        }
                      >
                        {t("overtimeCeiling.edit")}
                      </Button>
                    </PermissionGate>
                  </td>
                </tr>
              ),
            )}
          </tbody>
        </table>
      )}

      <PermissionGate permission={P.PLAN_WINDOWS}>
        <div className="flex flex-wrap items-end gap-2 border-t border-surface-border pt-3">
          <div>
            <Label htmlFor={`overtime-start-${teamId}`}>{t("overtimeCeiling.from")}</Label>
            <Input
              id={`overtime-start-${teamId}`}
              type="date"
              className="h-8 w-40"
              value={draft.start}
              onChange={(e) => setDraft({ ...draft, start: e.target.value })}
            />
          </div>
          <div>
            <Label htmlFor={`overtime-end-${teamId}`}>{t("overtimeCeiling.toOptional")}</Label>
            <Input
              id={`overtime-end-${teamId}`}
              type="date"
              className="h-8 w-40"
              min={draft.start}
              value={draft.end}
              onChange={(e) => setDraft({ ...draft, end: e.target.value })}
            />
          </div>
          <div>
            <Label htmlFor={`overtime-hours-${teamId}`}>{t("overtimeCeiling.hoursPerDay")}</Label>
            <Input
              id={`overtime-hours-${teamId}`}
              inputMode="decimal"
              className="h-8 w-24 text-right"
              value={draft.hours}
              onChange={(e) => setDraft({ ...draft, hours: e.target.value })}
            />
          </div>
          <Button
            type="button"
            size="sm"
            disabled={busy || draft.start === "" || draftHours == null}
            onClick={() =>
              void run(
                () =>
                  createCapacityRule({
                    team_id: teamId,
                    effective_start: draft.start,
                    effective_end: draft.end || null,
                    max_overtime_hours_per_day: draftHours ?? 0,
                  }),
                t("overtimeCeiling.created"),
              ).then((ok) => ok && setDraft({ start: today(), end: "", hours: "" }))
            }
          >
            {t("overtimeCeiling.add")}
          </Button>
        </div>
      </PermissionGate>
      {formError ? (
        <p role="alert" className="text-sm text-status-danger">
          {formError}
        </p>
      ) : null}
    </div>
  );
}
