/**
 * Inspection → reliability signals per equipment (rolling window). Equipment is shown by
 * code and name (linked to its record), never by id.
 */
import type { ColumnDef } from "@tanstack/react-table";
import { Loader2, RefreshCw } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { LinkedEntityBadge } from "@/components/common/LinkedEntityBadge";
import { DataTable } from "@/components/data/DataTable";
import { Button, Input, Label } from "@/components/ui";
import { ErrorState } from "@/components/ui/data-state";
import { EmptyState } from "@/components/ui/empty-state";
import { useFormatters } from "@/hooks/use-formatters";
import { usePermissions } from "@/hooks/use-permissions";
import {
  listInspectionReliabilitySignals,
  refreshInspectionReliabilitySignals,
} from "@/services/inspection-service";
import type { InspectionReliabilitySignal } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

/** Backend window range (`signals::refresh_inspection_reliability_signals` clamps to it). */
const MAX_WINDOW_DAYS = 366;

export function InspectionSignalsPanel({ refreshKey }: { refreshKey: number }) {
  const { t } = useTranslation("inspections");
  const { can } = usePermissions();
  const { formatDate, formatNumber } = useFormatters();
  const [signals, setSignals] = useState<InspectionReliabilitySignal[]>([]);
  const [windowDays, setWindowDays] = useState("30");
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const [windowError, setWindowError] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      setSignals(await listInspectionReliabilitySignals({}));
      setError(null);
    } catch (err) {
      setError(err);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load, refreshKey]);

  const refresh = async () => {
    const days = Number(windowDays);
    if (!Number.isInteger(days) || days < 1 || days > MAX_WINDOW_DAYS) {
      setWindowError(true);
      return;
    }
    setWindowError(false);
    setBusy(true);
    try {
      await refreshInspectionReliabilitySignals({ window_days: days });
      await load();
    } catch {
      // Reported by the global IPC toast.
    } finally {
      setBusy(false);
    }
  };

  const columns = useMemo<ColumnDef<InspectionReliabilitySignal>[]>(
    () => [
      {
        id: "equipment",
        accessorFn: (s) => s.asset_code ?? "",
        header: t("signals.equipment"),
        cell: ({ row }) => (
          <span className="flex flex-wrap items-center gap-1.5">
            <LinkedEntityBadge
              entity="asset"
              code={row.original.asset_code}
              entityId={row.original.equipment_id}
            />
            <span className="text-sm">{row.original.asset_name}</span>
          </span>
        ),
      },
      {
        id: "period",
        accessorFn: (s) => s.period_end,
        header: t("signals.period"),
        cell: ({ row }) => (
          <span className="text-xs">
            {formatDate(row.original.period_start)} – {formatDate(row.original.period_end)}
          </span>
        ),
      },
      { accessorKey: "warning_count", header: t("signals.warnings") },
      { accessorKey: "fail_count", header: t("signals.fails") },
      { accessorKey: "anomaly_open_count", header: t("signals.openAnomalies") },
      {
        id: "coverage",
        accessorFn: (s) => s.checkpoint_coverage_ratio,
        header: t("signals.coverage"),
        cell: ({ row }) => (
          <span>
            {formatNumber(row.original.checkpoint_coverage_ratio * 100, {
              maximumFractionDigits: 0,
            })}{" "}
            %
          </span>
        ),
      },
    ],
    [formatDate, formatNumber, t],
  );

  return (
    <div className="space-y-4" data-testid="inspection-signals">
      <p className="text-sm text-text-muted">{t("signals.description")}</p>
      {can(P.INS_ADMIN) ? (
        <div className="flex flex-wrap items-end gap-3">
          <div className="space-y-1">
            <Label htmlFor="signals-window">{t("signals.window")}</Label>
            <Input
              id="signals-window"
              inputMode="numeric"
              className="h-11 w-28"
              aria-invalid={windowError}
              value={windowDays}
              onChange={(e) => setWindowDays(e.target.value)}
            />
          </div>
          <Button variant="outline" className="h-11" disabled={busy} onClick={() => void refresh()}>
            {busy ? (
              <Loader2 className="mr-2 h-4 w-4 animate-spin" aria-hidden />
            ) : (
              <RefreshCw className="mr-2 h-4 w-4" aria-hidden />
            )}
            {t("signals.refresh")}
          </Button>
          {windowError ? (
            <p role="alert" className="text-sm text-tone-danger-fg">
              {t("signals.invalidWindow", { max: MAX_WINDOW_DAYS })}
            </p>
          ) : null}
        </div>
      ) : null}
      {error != null && signals.length === 0 ? (
        <ErrorState error={error} onRetry={() => void load()} retrying={loading} />
      ) : !loading && signals.length === 0 ? (
        <EmptyState title={t("signals.empty")} description={t("signals.emptyHint")} />
      ) : (
        <DataTable
          columns={columns}
          data={signals}
          pageSize={20}
          isLoading={loading}
          skeletonRows={4}
        />
      )}
    </div>
  );
}
