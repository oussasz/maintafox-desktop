import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { mfCard, mfLayout, mfTable } from "@/design-system/tokens";
import { cn } from "@/lib/utils";
import {
  getFmecaSeverityOccurrenceMatrix,
  getRamFmecaRpnCriticalThreshold,
  listFmecaItemsForEquipment,
} from "@/services/reliability-service";
import type { FmecaItemWithContext, FmecaSeverityOccurrenceMatrix } from "@shared/ipc-types";

import { useRequiredRamsEquipmentId } from "./rams-equipment-context";

function cellStyle(count: number, maxCount: number, active: boolean): string {
  if (maxCount <= 0) {
    return "border-surface-border bg-surface-2 text-text-muted";
  }
  const t = count / maxCount;
  const bg =
    count === 0
      ? "bg-surface-2"
      : t < 0.34
        ? "bg-primary/15"
        : t < 0.67
          ? "bg-primary/28"
          : "bg-primary/40";
  return cn(
    "border border-surface-border text-text-primary",
    bg,
    active && "ring-2 ring-primary ring-offset-2 ring-offset-surface-0",
  );
}

export function ReliabilityAdvancedPage() {
  const { t } = useTranslation("reliability");
  const equipmentId = useRequiredRamsEquipmentId();
  const [rpnThreshold, setRpnThreshold] = useState(150);
  const [matrix, setMatrix] = useState<FmecaSeverityOccurrenceMatrix | null>(null);
  const [rows, setRows] = useState<FmecaItemWithContext[]>([]);
  const [sel, setSel] = useState<{ s: number; o: number } | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    void getRamFmecaRpnCriticalThreshold()
      .then(setRpnThreshold)
      .catch(() => {
        /* keep default */
      });
  }, []);

  const maxCount = useMemo(() => {
    if (!matrix?.cells.length) {
      return 0;
    }
    return Math.max(...matrix.cells.map((c) => c.count), 0);
  }, [matrix]);

  const loadMatrix = useCallback(async () => {
    setErr(null);
    setLoading(true);
    try {
      const m = await getFmecaSeverityOccurrenceMatrix(equipmentId);
      setMatrix(m);
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
      setMatrix(null);
    } finally {
      setLoading(false);
    }
  }, [equipmentId]);

  const loadRows = useCallback(async () => {
    setErr(null);
    try {
      const list = await listFmecaItemsForEquipment({
        equipment_id: equipmentId,
        severity: sel?.s ?? null,
        occurrence: sel?.o ?? null,
        limit: 500,
      });
      setRows(list);
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
      setRows([]);
    }
  }, [equipmentId, sel]);

  useEffect(() => {
    void loadMatrix();
  }, [loadMatrix]);

  useEffect(() => {
    void loadRows();
  }, [loadRows]);

  const countAt = (s: number, o: number) =>
    matrix?.cells.find((c) => c.severity === s && c.occurrence === o)?.count ?? 0;

  return (
    <div className={cn(mfLayout.moduleWorkspaceBody, "space-y-4 text-sm text-text-primary")}>
      <div>
        <h2 className="text-lg font-semibold text-text-primary">{t("advanced.title")}</h2>
        <p className="mt-1 max-w-prose text-xs text-text-muted">{t("advanced.heatmapHint")}</p>
      </div>

      <div className="flex flex-wrap items-end gap-3">
        <button
          type="button"
          className="rounded-md border border-surface-border bg-surface-2 px-3 py-1.5 text-xs text-text-primary hover:bg-surface-3"
          onClick={() => {
            void loadMatrix();
            void loadRows();
          }}
        >
          {t("advanced.refresh")}
        </button>
        {sel ? (
          <button
            type="button"
            className="rounded-md border border-surface-border px-3 py-1.5 text-xs text-text-secondary hover:bg-surface-2"
            onClick={() => setSel(null)}
          >
            {t("advanced.clearFilter")}
          </button>
        ) : null}
      </div>

      {err ? <p className="text-sm text-text-danger">{err}</p> : null}
      {loading ? <p className="text-xs text-text-muted">{t("advanced.loading")}</p> : null}

      <div className={cn(mfCard.panelMuted, "overflow-x-auto")}>
        <p className="mb-2 text-center text-[11px] font-medium text-text-muted">
          {t("advanced.heatmapTitle")}
        </p>
        <div
          className="mx-auto grid w-max gap-0.5"
          style={{ gridTemplateColumns: `36px repeat(10, minmax(30px, 1fr))` }}
        >
          <div />
          {Array.from({ length: 10 }, (_, i) => (
            <div key={`o-h-${i + 1}`} className="text-center text-[9px] text-text-muted">
              O{i + 1}
            </div>
          ))}
          {Array.from({ length: 10 }, (_, si) => {
            const s = si + 1;
            return (
              <div key={`row-${s}`} className="contents">
                <div className="flex items-center pr-1 text-[9px] text-text-muted">S{s}</div>
                {Array.from({ length: 10 }, (_, oi) => {
                  const o = oi + 1;
                  const cnt = countAt(s, o);
                  const active = sel?.s === s && sel?.o === o;
                  return (
                    <button
                      key={`c-${s}-${o}`}
                      type="button"
                      title={`S=${s} O=${o} count=${cnt}`}
                      className={cn(
                        "h-8 min-w-[30px] rounded-sm text-[10px] font-mono transition-transform hover:z-10 focus:outline-none focus-visible:ring-2 focus-visible:ring-primary",
                        cellStyle(cnt, maxCount, active),
                      )}
                      onClick={() => setSel({ s, o })}
                    >
                      {cnt}
                    </button>
                  );
                })}
              </div>
            );
          })}
        </div>
        <div className="mt-3 flex flex-wrap gap-4 text-xs text-text-muted">
          <span>
            {t("advanced.sAxis")} / {t("advanced.oAxis")}
          </span>
          {sel ? (
            <span className="text-text-primary">
              {t("advanced.selected")}: S={sel.s} · O={sel.o}
            </span>
          ) : (
            <span>{t("advanced.filterHint")}</span>
          )}
        </div>
      </div>

      <div className={mfCard.panel}>
        <h3 className="mb-2 text-sm font-semibold text-text-primary">{t("advanced.tableTitle")}</h3>
        {rows.length === 0 ? (
          <p className="text-sm text-text-muted">{t("advanced.noRows")}</p>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full min-w-[720px] border-collapse text-left text-xs">
              <thead>
                <tr className={mfTable.header}>
                  <th className="px-2 py-2">{t("advanced.colAnalysis")}</th>
                  <th className="px-2 py-2">S</th>
                  <th className="px-2 py-2">O</th>
                  <th className="px-2 py-2">D</th>
                  <th className="px-2 py-2">RPN</th>
                  <th className="px-2 py-2">{t("advanced.colFailure")}</th>
                  <th className="px-2 py-2">
                    {t("advanced.colInventory", { threshold: rpnThreshold })}
                  </th>
                </tr>
              </thead>
              <tbody>
                {rows.map((r) => (
                  <tr key={r.id} className={cn("border-b border-surface-border", mfTable.rowHover)}>
                    <td className="px-2 py-1.5 text-text-secondary">{r.analysis_title}</td>
                    <td className="px-2 py-1.5 font-mono">{r.severity}</td>
                    <td className="px-2 py-1.5 font-mono">{r.occurrence}</td>
                    <td className="px-2 py-1.5 font-mono">{r.detectability}</td>
                    <td className="px-2 py-1.5 font-mono">{r.rpn}</td>
                    <td className="max-w-[200px] truncate px-2 py-1.5 text-text-primary">
                      {r.functional_failure}
                    </td>
                    <td className="px-2 py-1.5">
                      {r.rpn > rpnThreshold ? (
                        <span
                          className={cn(
                            r.inventory_status === "critical_shortage" &&
                              "font-medium text-text-danger",
                            r.inventory_status === "ok" && "text-text-success",
                            r.inventory_status === "no_wo_link" && "text-text-muted",
                            r.inventory_status === "no_spare_lines" && "text-text-warning",
                            r.inventory_status === "not_applicable" && "text-text-muted",
                          )}
                        >
                          {r.inventory_status === "critical_shortage"
                            ? t("advanced.invCritical")
                            : r.inventory_status === "ok"
                              ? t("advanced.invOk", { qty: r.spare_stock_total?.toFixed(2) ?? "0" })
                              : r.inventory_status === "no_wo_link"
                                ? t("advanced.invNoWo")
                                : r.inventory_status === "no_spare_lines"
                                  ? t("advanced.invNoParts")
                                  : r.inventory_status === "not_applicable"
                                    ? "—"
                                    : r.inventory_status}
                        </span>
                      ) : (
                        <span className="text-text-muted">—</span>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>
    </div>
  );
}
