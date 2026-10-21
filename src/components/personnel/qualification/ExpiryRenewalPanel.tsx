/**
 * Expiry follow-up and renewal plan: every certification that has run out, runs out within the
 * chosen horizon or still waits for verification, grouped by certification so a training
 * session can be planned for everyone who needs it.
 */
import { CalendarClock, GraduationCap } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { EmptyState } from "@/components/ui/empty-state";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { StatusChip } from "@/components/ui/status-chip";
import { usePermissions } from "@/hooks/use-permissions";
import {
  type QualificationMatrix,
  type QualificationMatrixColumn,
  getQualificationMatrix,
} from "@/services/qualification-gate-service";
import { P } from "@shared/rbac/permissions.generated";

import { PersonQualificationDialog } from "./PersonQualificationSheet";
import { daysUntil, MATRIX_CELL_TONE, todayIso } from "./qualification-tones";

export const EXPIRY_HORIZONS = [30, 60, 90, 180, 365] as const;

interface DueRow {
  personnel_id: number;
  full_name: string;
  employee_code: string;
  team_or_company: string | null;
  column: QualificationMatrixColumn;
  status: "expired" | "expiring_soon" | "pending" | "rejected" | "valid";
  expires_at: string | null;
  days_left: number | null;
}

export interface ExpiryRenewalPanelProps {
  refreshKey?: number;
  /** Opens the training-session form for a certification (renewal session). */
  onPlanSession?: (certificationTypeId: number) => void;
}

export function ExpiryRenewalPanel({ refreshKey = 0, onPlanSession }: ExpiryRenewalPanelProps) {
  const { t } = useTranslation("training");
  const { can } = usePermissions();
  const [matrix, setMatrix] = useState<QualificationMatrix | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<unknown>(null);
  const [horizon, setHorizon] = useState<number>(90);
  const [open, setOpen] = useState<DueRow | null>(null);
  const today = todayIso();

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setMatrix(await getQualificationMatrix({}));
    } catch (err) {
      setError(err);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load, refreshKey]);

  const groups = useMemo(() => {
    if (!matrix) return [];
    const columns = new Map(
      matrix.columns.filter((c) => c.kind === "certification").map((c) => [c.key, c]),
    );
    const due: DueRow[] = [];
    for (const row of matrix.rows) {
      for (const cell of row.cells) {
        const column = columns.get(cell.key);
        if (!column) continue;
        const daysLeft = cell.expires_at ? daysUntil(cell.expires_at, today) : null;
        const inHorizon = daysLeft != null && daysLeft <= horizon;
        if (cell.status === "valid" && !inHorizon) continue;
        if (cell.status === "unvalidated") continue;
        due.push({
          personnel_id: row.personnel_id,
          full_name: row.full_name,
          employee_code: row.employee_code,
          team_or_company: row.company_name ?? row.team_name,
          column,
          status: cell.status as DueRow["status"],
          expires_at: cell.expires_at,
          days_left: daysLeft,
        });
      }
    }
    const byType = new Map<string, { column: QualificationMatrixColumn; rows: DueRow[] }>();
    for (const d of due) {
      const g = byType.get(d.column.key) ?? { column: d.column, rows: [] };
      g.rows.push(d);
      byType.set(d.column.key, g);
    }
    return [...byType.values()]
      .map((g) => ({
        ...g,
        rows: g.rows.sort((a, b) => (a.days_left ?? 99999) - (b.days_left ?? 99999)),
      }))
      .sort((a, b) => (a.rows[0]?.days_left ?? 99999) - (b.rows[0]?.days_left ?? 99999));
  }, [matrix, horizon, today]);

  const dueLabel = (r: DueRow): string => {
    if (r.status === "pending") return t("expiry.awaitingVerification");
    if (r.days_left == null) return t("expiry.unreadable");
    if (r.days_left < 0) return t("expiry.expiredDaysAgo", { count: -r.days_left });
    if (r.days_left === 0) return t("expiry.expiresToday");
    return t("expiry.expiresInDays", { count: r.days_left });
  };

  if (error && !matrix)
    return <ErrorState error={error} onRetry={() => void load()} retrying={loading} />;
  if (loading && !matrix) return <LoadingState />;

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center gap-3">
        <label htmlFor="expiry-horizon" className="text-sm text-text-muted">
          {t("expiry.horizon")}
        </label>
        <Select value={String(horizon)} onValueChange={(v) => setHorizon(Number(v))}>
          <SelectTrigger id="expiry-horizon" className="w-48">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {EXPIRY_HORIZONS.map((h) => (
              <SelectItem key={h} value={String(h)}>
                {t("expiry.nextDays", { count: h })}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <span className="text-sm text-text-muted">{t("expiry.intro")}</span>
      </div>

      {groups.length === 0 ? (
        <EmptyState
          icon={<CalendarClock className="h-8 w-8" aria-hidden />}
          title={t("expiry.empty")}
          description={t("expiry.emptyHint", { count: horizon })}
        />
      ) : (
        groups.map((g) => (
          <section key={g.column.key} className="rounded-md border border-surface-border">
            <header className="flex flex-wrap items-center justify-between gap-2 border-b border-surface-border bg-surface-1 px-3 py-2">
              <h3 className="text-sm font-semibold text-text-primary">
                {g.column.label}
                <span className="ml-2 font-normal text-text-muted">
                  {t("expiry.peopleCount", { count: g.rows.length })}
                </span>
              </h3>
              {onPlanSession && can(P.TRN_MANAGE) ? (
                <Button
                  size="sm"
                  variant="outline"
                  className="gap-1"
                  onClick={() => onPlanSession(g.column.id)}
                >
                  <GraduationCap className="h-3.5 w-3.5" aria-hidden />
                  {t("expiry.planSession")}
                </Button>
              ) : null}
            </header>
            <ul className="divide-y divide-surface-border">
              {g.rows.map((r) => (
                <li
                  key={`${r.personnel_id}-${r.column.key}`}
                  className="flex flex-wrap items-center gap-3 px-3 py-2 text-sm"
                >
                  <span className="min-w-[200px] flex-1">
                    <span className="font-medium text-text-primary">{r.full_name}</span>
                    <span className="ml-2 text-xs text-text-muted">
                      {[r.employee_code, r.team_or_company].filter(Boolean).join(" · ")}
                    </span>
                  </span>
                  <span className="tabular-nums text-text-secondary">
                    {r.expires_at?.slice(0, 10) ?? "—"}
                  </span>
                  <StatusChip
                    label={dueLabel(r)}
                    tone={
                      r.status === "valid"
                        ? MATRIX_CELL_TONE.expiring_soon
                        : MATRIX_CELL_TONE[r.status]
                    }
                    dot
                  />
                  {can(P.TRN_CERTIFY) ? (
                    <Button size="sm" variant="ghost" onClick={() => setOpen(r)}>
                      {r.status === "pending" ? t("sheet.verify") : t("sheet.renew")}
                    </Button>
                  ) : null}
                </li>
              ))}
            </ul>
          </section>
        ))
      )}

      {open ? (
        <PersonQualificationDialog
          open
          onOpenChange={(o) => !o && setOpen(null)}
          personName={open.full_name}
          personnelId={open.personnel_id}
          focusCertificationTypeId={open.status === "pending" ? null : open.column.id}
          onChanged={() => void load()}
        />
      ) : null}
    </div>
  );
}
