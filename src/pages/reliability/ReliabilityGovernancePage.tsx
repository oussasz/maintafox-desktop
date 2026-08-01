import type { Edge, Node } from "@xyflow/react";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { IshikawaDiagramCanvas } from "@/components/reliability/IshikawaDiagramCanvas";
import { mfLayout } from "@/design-system/tokens";
import { cn } from "@/lib/utils";
import { listRamIshikawaDiagramsByEquipments } from "@/services/reliability-service";

import { useRamsEquipment } from "./rams-equipment-context";

type CategoryKey = "machine" | "method" | "material" | "manpower" | "measurement" | "nature";

const CAT_NODE_TO_KEY: Record<string, CategoryKey> = {
  cat_machine: "machine",
  cat_method: "method",
  cat_material: "material",
  cat_manpower: "manpower",
  cat_measurement: "measurement",
  cat_nature: "nature",
};

function categoryCountFromFlow(flowJson: string): Record<CategoryKey, number> {
  const initial: Record<CategoryKey, number> = {
    machine: 0,
    method: 0,
    material: 0,
    manpower: 0,
    measurement: 0,
    nature: 0,
  };
  try {
    const parsed = JSON.parse(flowJson) as { nodes?: Node[]; edges?: Edge[] };
    const nodes = Array.isArray(parsed.nodes) ? parsed.nodes : [];
    const edges = Array.isArray(parsed.edges) ? parsed.edges : [];
    const nodeIds = new Set(nodes.map((n) => n.id));
    for (const e of edges) {
      const key = CAT_NODE_TO_KEY[e.source];
      if (!key) {
        continue;
      }
      if (!nodeIds.has(e.target)) {
        continue;
      }
      if (e.target === "effect") {
        continue;
      }
      initial[key] += 1;
    }
  } catch {
    /* ignore malformed legacy payload */
  }
  return initial;
}

export function ReliabilityGovernancePage() {
  const { t } = useTranslation("reliability");
  const { selectedEquipmentId, selectedEquipmentIds } = useRamsEquipment();
  const [compareLoading, setCompareLoading] = useState(false);
  const [compareError, setCompareError] = useState<string | null>(null);
  const [categorySummary, setCategorySummary] = useState<Record<CategoryKey, number> | null>(null);

  useEffect(() => {
    if (selectedEquipmentIds.length <= 1) {
      setCategorySummary(null);
      setCompareError(null);
      return;
    }
    let cancelled = false;
    void (async () => {
      setCompareLoading(true);
      setCompareError(null);
      try {
        const rows = await listRamIshikawaDiagramsByEquipments(selectedEquipmentIds, {
          limitPerEquipment: 1,
        });
        const totals: Record<CategoryKey, number> = {
          machine: 0,
          method: 0,
          material: 0,
          manpower: 0,
          measurement: 0,
          nature: 0,
        };
        for (const item of rows) {
          const first = item.diagrams[0];
          if (!first?.flow_json) {
            continue;
          }
          const one = categoryCountFromFlow(first.flow_json);
          totals.machine += one.machine;
          totals.method += one.method;
          totals.material += one.material;
          totals.manpower += one.manpower;
          totals.measurement += one.measurement;
          totals.nature += one.nature;
        }
        if (!cancelled) {
          setCategorySummary(totals);
        }
      } catch (e) {
        if (!cancelled) {
          setCompareError(e instanceof Error ? e.message : String(e));
          setCategorySummary(null);
        }
      } finally {
        if (!cancelled) {
          setCompareLoading(false);
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [selectedEquipmentIds]);

  const summaryRows = useMemo(
    () =>
      (
        [
          ["machine", "governance.categories.machine"],
          ["method", "governance.categories.method"],
          ["material", "governance.categories.material"],
          ["manpower", "governance.categories.manpower"],
          ["measurement", "governance.categories.measurement"],
          ["nature", "governance.categories.motherNature"],
        ] as const
      ).map(([key, labelKey]) => ({
        key,
        label: t(labelKey),
        value: categorySummary?.[key] ?? 0,
      })),
    [categorySummary, t],
  );

  return (
    <div className={cn(mfLayout.moduleWorkspaceBody, "flex flex-col gap-4")}>
      <div>
        <h2 className="text-lg font-semibold text-text-primary">{t("governance.title")}</h2>
        <p className="mt-2 max-w-prose text-sm text-text-muted">{t("governance.body")}</p>
      </div>
      {selectedEquipmentIds.length > 1 ? (
        <>
          <div className="rounded-md border border-surface-border bg-surface-1 p-3">
            <div className="mb-2 flex items-center justify-between">
              <p className="text-sm font-medium text-text-primary">
                {t("governance.comparisonTitle", { count: selectedEquipmentIds.length })}
              </p>
              {compareLoading ? (
                <span className="text-xs text-text-muted">{t("governance.comparisonLoading")}</span>
              ) : null}
            </div>
            {compareError ? <p className="mb-2 text-xs text-text-danger">{compareError}</p> : null}
            <div className="grid grid-cols-2 gap-2 md:grid-cols-3">
              {summaryRows.map((row) => (
                <div
                  key={row.key}
                  className="rounded border border-surface-border bg-surface-0 px-2 py-1.5"
                >
                  <div className="text-[10px] uppercase tracking-wide text-text-muted">
                    {row.label}
                  </div>
                  <div className="font-mono text-sm tabular-nums text-text-primary">
                    {row.value}
                  </div>
                </div>
              ))}
            </div>
          </div>

          <div className="grid grid-cols-1 gap-3 xl:grid-cols-2">
            {selectedEquipmentIds.map((equipmentId) => (
              <div
                key={equipmentId}
                className="rounded-md border border-surface-border bg-surface-1 p-2"
              >
                <p className="mb-2 font-mono text-xs text-text-secondary">
                  {t("governance.equip")} #{equipmentId}
                </p>
                <IshikawaDiagramCanvas equipmentId={equipmentId} />
              </div>
            ))}
          </div>
        </>
      ) : selectedEquipmentId != null ? (
        <IshikawaDiagramCanvas equipmentId={selectedEquipmentId} />
      ) : null}
    </div>
  );
}
