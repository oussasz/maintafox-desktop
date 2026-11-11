import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { mfInput } from "@/design-system/tokens";
import { cn } from "@/lib/utils";
import {
  type PmPlanCoverage,
  type PmPlanRow,
  getPmPlanCoverage,
  listPmPlans,
  setPmPlanCoveredBy,
} from "@/services/pm-service";
import { usePmStore } from "@/stores/pm-store";

import { pmUnreportedErrorText } from "./pm-errors";

interface PmPlanCoveragePanelProps {
  plan: PmPlanRow;
  /** `pm.edit` at the plan's site. */
  canEdit: boolean;
}

/**
 * Nested plans (L1-15): the annual overhaul can cover the weekly check of the same asset, so the
 * asset gets one work order that week, not two. Shown for a plan on one asset only.
 */
export function PmPlanCoveragePanel({ plan, canEdit }: PmPlanCoveragePanelProps) {
  const { t } = useTranslation("pm");
  const [coverage, setCoverage] = useState<PmPlanCoverage | null>(null);
  const [candidates, setCandidates] = useState<PmPlanRow[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const openPlan = usePmStore((s) => s.openPlan);

  const load = useCallback(async () => {
    try {
      const [cov, plans] = await Promise.all([getPmPlanCoverage(plan.id), listPmPlans({})]);
      setCoverage(cov);
      setCandidates(
        plans.filter(
          (p) =>
            p.id !== plan.id &&
            p.asset_scope_type === "equipment" &&
            p.asset_scope_id === plan.asset_scope_id,
        ),
      );
    } catch (err) {
      setError(pmUnreportedErrorText(err));
    }
  }, [plan.id, plan.asset_scope_id]);

  useEffect(() => {
    void load();
  }, [load]);

  async function handleChange(value: string) {
    setBusy(true);
    setError(null);
    try {
      setCoverage(await setPmPlanCoveredBy(plan.id, value === "" ? null : Number(value)));
      // The plan's change counter moved: reload it so its other actions are not refused as stale.
      await openPlan(plan.id);
    } catch (err) {
      setError(pmUnreportedErrorText(err));
    } finally {
      setBusy(false);
    }
  }

  if (plan.asset_scope_type !== "equipment" || coverage == null) return null;
  const covers = coverage.covers;
  if (!canEdit && coverage.covered_by == null && covers.length === 0) return null;

  return (
    <section className="space-y-2 rounded-lg border p-4 text-sm">
      <h3 className="font-medium">{t("coverage.title")}</h3>
      <p className="text-xs text-text-muted">{t("coverage.hint")}</p>
      <div className="flex flex-wrap items-center gap-2">
        <label htmlFor="pm-plan-covered-by" className="text-text-muted">
          {t("coverage.coveredBy")}
        </label>
        <select
          id="pm-plan-covered-by"
          value={coverage.covered_by?.id ?? ""}
          onChange={(e) => void handleChange(e.target.value)}
          disabled={!canEdit || busy}
          className={cn(mfInput.nativeSelect, "h-9 max-w-xs")}
        >
          <option value="">{t("coverage.none")}</option>
          {candidates.map((p) => (
            <option key={p.id} value={p.id}>
              {p.code} — {p.title}
            </option>
          ))}
        </select>
      </div>
      {covers.length > 0 ? (
        <p>{t("coverage.covers", { plans: covers.map((c) => c.code).join(", ") })}</p>
      ) : null}
      {error ? (
        <p role="alert" className="text-xs text-tone-danger-fg">
          {error}
        </p>
      ) : null}
    </section>
  );
}
