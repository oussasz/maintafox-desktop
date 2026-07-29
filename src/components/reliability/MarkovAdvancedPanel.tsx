import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { mfCard } from "@/design-system/tokens";
import { usePermissions } from "@/hooks/use-permissions";
import {
  createMarkovModel,
  deleteMarkovModel,
  evaluateMarkovModel,
  listMarkovModels,
  updateMarkovModel,
} from "@/services/reliability-service";
import type { MarkovModel } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

const DEFAULT_MARKOV_GRAPH = `{"spec_version":1,"kind":"discrete","states":["Up","Degraded","Down"],"matrix":[[0.94,0.05,0.01],[0.10,0.75,0.15],[0.25,0.50,0.25]]}`;

type MarkovAdvancedPanelProps = {
  equipmentId: number;
};

export function MarkovAdvancedPanel({ equipmentId }: MarkovAdvancedPanelProps) {
  const { t } = useTranslation("reliability");
  const { can, isLoading: permissionsLoading } = usePermissions();
  const canAnalyze = can(P.RAM_ANALYZE);

  const [rows, setRows] = useState<MarkovModel[]>([]);
  const [selected, setSelected] = useState<MarkovModel | null>(null);
  const [title, setTitle] = useState("Modèle Markov équipement");
  const [graphJson, setGraphJson] = useState(DEFAULT_MARKOV_GRAPH);
  const [err, setErr] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const loadRows = useCallback(async () => {
    if (!can(P.RAM_VIEW)) {
      setRows([]);
      return;
    }
    try {
      const list = await listMarkovModels({ equipment_id: equipmentId, limit: 20 });
      setRows(list);
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
      setRows([]);
    }
  }, [can, equipmentId]);

  useEffect(() => {
    void loadRows();
  }, [loadRows]);

  const onCreate = async () => {
    if (!canAnalyze) {
      return;
    }
    setErr(null);
    setLoading(true);
    try {
      JSON.parse(graphJson);
      await createMarkovModel({
        equipment_id: equipmentId,
        title: title.trim() || "Modèle Markov",
        graph_json: graphJson,
        status: "draft",
      });
      await loadRows();
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  };

  const onSave = async () => {
    if (!canAnalyze || selected == null) {
      return;
    }
    setErr(null);
    setLoading(true);
    try {
      JSON.parse(graphJson);
      const updated = await updateMarkovModel({
        id: selected.id,
        expected_row_version: selected.row_version,
        graph_json: graphJson,
        title: title.trim() || selected.title,
      });
      setSelected(updated);
      await loadRows();
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  };

  const onEvaluate = async () => {
    if (!canAnalyze || selected == null) {
      return;
    }
    setErr(null);
    setLoading(true);
    try {
      const updated = await evaluateMarkovModel(selected.id);
      setSelected(updated);
      await loadRows();
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  };

  const onDelete = async () => {
    if (!canAnalyze || selected == null) {
      return;
    }
    setErr(null);
    setLoading(true);
    try {
      await deleteMarkovModel(selected.id);
      setSelected(null);
      setGraphJson(DEFAULT_MARKOV_GRAPH);
      await loadRows();
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  };

  return (
    <section className={mfCard.panel}>
      <h2 className="mb-1 text-base font-medium text-text-primary">{t("advanced.markovTitle")}</h2>
      <p className="mb-3 text-[11px] text-text-muted">{t("advanced.markovHint")}</p>

      {!canAnalyze && !permissionsLoading ? (
        <p className="mb-3 rounded border border-status-warning/30 bg-status-warning/10 px-3 py-2 text-xs text-status-warning">
          {t("advanced.markovPermission")}
        </p>
      ) : null}

      {err ? <p className="mb-3 text-xs text-text-danger">{err}</p> : null}

      <div className="mb-3 flex flex-wrap gap-2">
        <input
          type="text"
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          className="min-w-[12rem] flex-1 rounded border border-surface-border bg-surface-1 px-2 py-1 text-xs"
          placeholder={t("advanced.markovTitlePlaceholder")}
        />
        <button
          type="button"
          disabled={!canAnalyze || loading}
          className="rounded border border-surface-border bg-surface-2 px-2 py-1 text-xs disabled:opacity-40"
          onClick={() => void onCreate()}
        >
          {t("advanced.markovCreate")}
        </button>
        <button
          type="button"
          disabled={!canAnalyze || selected == null || loading}
          className="rounded border border-surface-border bg-surface-2 px-2 py-1 text-xs disabled:opacity-40"
          onClick={() => void onSave()}
        >
          {t("advanced.markovSave")}
        </button>
        <button
          type="button"
          disabled={!canAnalyze || selected == null || loading}
          className="rounded border border-primary/40 bg-primary/10 px-2 py-1 text-xs text-text-primary disabled:opacity-40"
          onClick={() => void onEvaluate()}
        >
          {t("advanced.markovEvaluate")}
        </button>
        <button
          type="button"
          disabled={!canAnalyze || selected == null || loading}
          className="rounded border border-status-danger/40 px-2 py-1 text-xs text-status-danger disabled:opacity-40"
          onClick={() => void onDelete()}
        >
          {t("advanced.markovDelete")}
        </button>
      </div>

      <div className="mb-3 overflow-auto rounded border border-surface-border">
        <table className="w-full text-left text-[11px]">
          <thead className="bg-surface-2 text-text-muted">
            <tr>
              <th className="px-2 py-1">ID</th>
              <th className="px-2 py-1">{t("advanced.markovColTitle")}</th>
              <th className="px-2 py-1">{t("advanced.markovColStatus")}</th>
            </tr>
          </thead>
          <tbody>
            {rows.length === 0 ? (
              <tr>
                <td colSpan={3} className="px-2 py-3 text-text-muted">
                  {t("advanced.markovNone")}
                </td>
              </tr>
            ) : (
              rows.map((row) => (
                <tr
                  key={row.id}
                  className={`cursor-pointer border-t border-surface-border ${selected?.id === row.id ? "bg-primary/10" : "hover:bg-surface-2"}`}
                  onClick={() => {
                    setSelected(row);
                    setTitle(row.title);
                    setGraphJson(row.graph_json);
                  }}
                >
                  <td className="px-2 py-1 font-mono">{row.id}</td>
                  <td className="px-2 py-1">{row.title}</td>
                  <td className="px-2 py-1">{row.status}</td>
                </tr>
              ))
            )}
          </tbody>
        </table>
      </div>

      <label className="flex flex-col gap-1 text-[11px] text-text-secondary">
        {t("advanced.markovGraphLabel")}
        <textarea
          value={graphJson}
          onChange={(e) => setGraphJson(e.target.value)}
          className="h-32 rounded border border-surface-border bg-surface-1 p-2 font-mono text-[10px] text-text-primary"
          spellCheck={false}
        />
      </label>

      {selected?.result_json ? (
        <pre className="mt-3 max-h-28 overflow-auto rounded border border-surface-border bg-surface-2/60 p-2 text-[10px] text-text-muted">
          {selected.result_json}
        </pre>
      ) : null}
    </section>
  );
}
