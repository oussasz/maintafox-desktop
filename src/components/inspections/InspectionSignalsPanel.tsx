/**
 * Inspection → reliability signals per equipment (rolling window). Equipment is shown by
 * code and name, never by id.
 */
import { Loader2, RefreshCw } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button, Input, Label } from "@/components/ui";
import { useFormatters } from "@/hooks/use-formatters";
import { usePermissions } from "@/hooks/use-permissions";
import {
  listInspectionReliabilitySignals,
  refreshInspectionReliabilitySignals,
} from "@/services/inspection-service";
import { toErrorMessage } from "@/utils/errors";
import type { InspectionReliabilitySignal } from "@shared/ipc-types";

export function InspectionSignalsPanel() {
  const { t } = useTranslation("inspections");
  const { can } = usePermissions();
  const { formatDate, formatNumber } = useFormatters();
  const [signals, setSignals] = useState<InspectionReliabilitySignal[]>([]);
  const [windowDays, setWindowDays] = useState("30");
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      setSignals(await listInspectionReliabilitySignals({}));
      setError(null);
    } catch (err) {
      setError(t("errors.load", { message: toErrorMessage(err) }));
    } finally {
      setLoading(false);
    }
  }, [t]);

  useEffect(() => {
    void load();
  }, [load]);

  const refresh = async () => {
    const days = Number.parseInt(windowDays, 10);
    if (!Number.isFinite(days) || days < 1) {
      setError(t("signals.invalidWindow"));
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await refreshInspectionReliabilitySignals({ window_days: days });
      await load();
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="space-y-4">
      <p className="text-sm text-text-muted">{t("signals.description")}</p>
      {can("ins.admin") ? (
        <div className="flex flex-wrap items-end gap-3">
          <div className="space-y-1">
            <Label htmlFor="signals-window">{t("signals.window")}</Label>
            <Input
              id="signals-window"
              inputMode="numeric"
              className="w-28"
              value={windowDays}
              onChange={(e) => setWindowDays(e.target.value)}
            />
          </div>
          <Button size="lg" variant="outline" disabled={busy} onClick={() => void refresh()}>
            {busy ? (
              <Loader2 className="mr-2 h-4 w-4 animate-spin" />
            ) : (
              <RefreshCw className="mr-2 h-4 w-4" />
            )}
            {t("signals.refresh")}
          </Button>
        </div>
      ) : null}
      {error ? (
        <p role="alert" className="text-sm text-status-danger">
          {error}
        </p>
      ) : null}
      {loading ? (
        <Loader2 className="h-5 w-5 animate-spin text-text-muted" />
      ) : signals.length === 0 ? (
        <p className="text-sm text-text-muted">{t("signals.empty")}</p>
      ) : (
        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead className="text-left text-text-muted">
              <tr>
                <th className="py-2 pr-4">{t("signals.equipment")}</th>
                <th className="py-2 pr-4">{t("signals.period")}</th>
                <th className="py-2 pr-4">{t("signals.warnings")}</th>
                <th className="py-2 pr-4">{t("signals.fails")}</th>
                <th className="py-2 pr-4">{t("signals.openAnomalies")}</th>
                <th className="py-2 pr-4">{t("signals.coverage")}</th>
              </tr>
            </thead>
            <tbody>
              {signals.map((s) => (
                <tr key={s.id} className="border-t border-surface-border">
                  <td className="py-2 pr-4">
                    <span className="font-mono">{s.asset_code ?? "—"}</span> {s.asset_name}
                  </td>
                  <td className="py-2 pr-4">
                    {formatDate(s.period_start)} – {formatDate(s.period_end)}
                  </td>
                  <td className="py-2 pr-4">{s.warning_count}</td>
                  <td className="py-2 pr-4">{s.fail_count}</td>
                  <td className="py-2 pr-4">{s.anomaly_open_count}</td>
                  <td className="py-2 pr-4">
                    {formatNumber(s.checkpoint_coverage_ratio * 100, { maximumFractionDigits: 0 })}{" "}
                    %
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
