/**
 * WoCostSummaryCard.tsx
 *
 * Inline cost display for WO detail. Shows labor / parts / services / total
 * with variance indicator (expected vs actual duration).
 * Phase 2 – Sub-phase 05 – File 03 – Sprint S4.
 */

import { ArrowDown, ArrowUp, Minus } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Card, CardContent } from "@/components/ui/card";
import { getCostSummary } from "@/services/wo-service";
import type { WoCostSummary } from "@shared/ipc-types";

// ── Props ───────────────────────────────────────────────────────────────────

interface WoCostSummaryCardProps {
  woId: number;
  status: string;
}

// ── Component ───────────────────────────────────────────────────────────────

export function WoCostSummaryCard({ woId, status }: WoCostSummaryCardProps) {
  const { t } = useTranslation("ot");
  const [cost, setCost] = useState<WoCostSummary | null>(null);
  const [loading, setLoading] = useState(false);

  // biome-ignore lint/correctness/useExhaustiveDependencies: status triggers re-fetch when WO status changes
  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    getCostSummary(woId)
      .then((c) => {
        if (!cancelled) setCost(c);
      })
      .catch(() => {
        if (!cancelled) setCost(null);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [woId, status]);

  if (loading) {
    return (
      <Card>
        <CardContent className="p-3 text-xs text-muted-foreground">{t("cost.loading")}</CardContent>
      </Card>
    );
  }

  if (!cost) return null;

  const variance =
    cost.expected_duration_hours != null && cost.actual_duration_hours != null
      ? cost.actual_duration_hours - cost.expected_duration_hours
      : null;

  return (
    <Card>
      <CardContent className="p-3 space-y-2">
        <h4 className="text-sm font-semibold">{t("cost.title")}</h4>
        <div className="grid grid-cols-2 gap-x-4 gap-y-1 text-xs">
          <CostRow label={t("cost.labor")} value={cost.labor_cost} />
          <CostRow label={t("cost.parts")} value={cost.parts_cost} />
          <CostRow label={t("cost.services")} value={cost.service_cost} />
          <div className="col-span-2 border-t border-surface-border pt-1 mt-1 flex items-center justify-between text-sm font-bold">
            <span>{t("cost.total")}</span>
            <span>{fmt(cost.total_cost)}</span>
          </div>
        </div>

        {variance !== null && (
          <div className="flex items-center gap-1 text-xs pt-1">
            <span className="text-muted-foreground">{t("cost.variance")}:</span>
            {variance > 0 && (
              <span className="flex items-center gap-0.5 text-red-600 font-medium">
                <ArrowUp className="h-3 w-3" />+{variance}h
              </span>
            )}
            {variance < 0 && (
              <span className="flex items-center gap-0.5 text-green-600 font-medium">
                <ArrowDown className="h-3 w-3" />
                {variance}h
              </span>
            )}
            {variance === 0 && (
              <span className="flex items-center gap-0.5 text-muted-foreground font-medium">
                <Minus className="h-3 w-3" />
                0h
              </span>
            )}
          </div>
        )}
      </CardContent>
    </Card>
  );
}

// ── Sub-components ──────────────────────────────────────────────────────────

function CostRow({ label, value }: { label: string; value: number }) {
  return (
    <>
      <span className="text-muted-foreground">{label}</span>
      <span className="text-right font-mono">{fmt(value)}</span>
    </>
  );
}

function fmt(v: number): string {
  return v.toLocaleString("fr-FR", { minimumFractionDigits: 2, maximumFractionDigits: 2 });
}
