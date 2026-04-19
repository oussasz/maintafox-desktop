import { ClipboardCheck } from "lucide-react";
import { useCallback, useEffect, useState } from "react";

import { ModulePageShell } from "@/components/layout/ModulePageShell";
import {
  createInspectionTemplate,
  deferInspectionAnomaly,
  enqueueInspectionOffline,
  listInspectionAnomalies,
  listInspectionCheckpoints,
  listInspectionOfflineQueue,
  listInspectionReliabilitySignals,
  listInspectionResults,
  listInspectionRounds,
  listInspectionTemplateVersions,
  listInspectionTemplates,
  recordInspectionResult,
  refreshInspectionReliabilitySignals,
  routeInspectionAnomalyToDi,
  routeInspectionAnomalyToWo,
  scheduleInspectionRound,
} from "@/services/inspection-service";
import type {
  InspectionAnomaly,
  InspectionCheckpoint,
  InspectionOfflineQueueItem,
  InspectionReliabilitySignal,
  InspectionResult,
  InspectionRound,
  InspectionTemplate,
  InspectionTemplateVersion,
} from "@shared/ipc-types";

export function InspectionsPage() {
  const [templates, setTemplates] = useState<InspectionTemplate[]>([]);
  const [versions, setVersions] = useState<InspectionTemplateVersion[]>([]);
  const [checkpoints, setCheckpoints] = useState<InspectionCheckpoint[]>([]);
  const [rounds, setRounds] = useState<InspectionRound[]>([]);
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [code, setCode] = useState("");
  const [name, setName] = useState("");
  const [scheduleAt, setScheduleAt] = useState("");
  const [selectedRoundId, setSelectedRoundId] = useState<number | null>(null);
  const [results, setResults] = useState<InspectionResult[]>([]);
  const [anomalies, setAnomalies] = useState<InspectionAnomaly[]>([]);
  const [offlineQueue, setOfflineQueue] = useState<InspectionOfflineQueueItem[]>([]);
  const [cpIdStr, setCpIdStr] = useState("");
  const [numValStr, setNumValStr] = useState("");
  const [resComment, setResComment] = useState("");
  const [offlinePayload, setOfflinePayload] = useState('{"op":"record_inspection_result"}');
  const [offlineTempId, setOfflineTempId] = useState("");
  const [woTypeIdStr, setWoTypeIdStr] = useState("1");
  const [reliabilitySignals, setReliabilitySignals] = useState<InspectionReliabilitySignal[]>([]);
  const [signalWindowDays, setSignalWindowDays] = useState("30");

  const loadLists = useCallback(async () => {
    setError(null);
    try {
      const [t, r] = await Promise.all([listInspectionTemplates(), listInspectionRounds()]);
      setTemplates(t);
      setRounds(r);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    void loadLists();
  }, [loadLists]);

  const loadReliabilitySignals = useCallback(async () => {
    setError(null);
    try {
      const s = await listInspectionReliabilitySignals({});
      setReliabilitySignals(s);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    void loadReliabilitySignals();
  }, [loadReliabilitySignals]);

  useEffect(() => {
    if (selectedId == null) {
      setVersions([]);
      setCheckpoints([]);
      return;
    }
    let cancelled = false;
    void (async () => {
      setError(null);
      try {
        const [v, tList] = await Promise.all([
          listInspectionTemplateVersions({ template_id: selectedId }),
          listInspectionTemplates(),
        ]);
        if (cancelled) {
          return;
        }
        setVersions(v);
        const cur = tList.find((x) => x.id === selectedId)?.current_version_id;
        if (cur != null) {
          const c = await listInspectionCheckpoints({ template_version_id: cur });
          if (!cancelled) {
            setCheckpoints(c);
          }
        } else {
          setCheckpoints([]);
        }
      } catch (e) {
        if (!cancelled) {
          setError(e instanceof Error ? e.message : String(e));
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [selectedId]);

  const loadExecution = useCallback(async () => {
    if (selectedRoundId == null) {
      setResults([]);
      setAnomalies([]);
      return;
    }
    setError(null);
    try {
      const [res, anom, off] = await Promise.all([
        listInspectionResults({ round_id: selectedRoundId }),
        listInspectionAnomalies({ round_id: selectedRoundId }),
        listInspectionOfflineQueue(),
      ]);
      setResults(res);
      setAnomalies(anom);
      setOfflineQueue(off);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [selectedRoundId]);

  useEffect(() => {
    void loadExecution();
  }, [loadExecution]);

  const onCreate = async () => {
    setError(null);
    try {
      await createInspectionTemplate({
        code: code.trim(),
        name: name.trim(),
        checkpoints: [
          {
            sequence_order: 1,
            checkpoint_code: "DEFAULT-1",
            check_type: "observation",
          },
        ],
      });
      setCode("");
      setName("");
      await loadLists();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onSchedule = async () => {
    if (selectedId == null) {
      return;
    }
    setError(null);
    try {
      const r = await scheduleInspectionRound({
        template_id: selectedId,
        scheduled_at: scheduleAt.trim() || null,
        assigned_to_id: null,
      });
      setScheduleAt("");
      setSelectedRoundId(r.id);
      await loadLists();
      await loadExecution();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onRecordResult = async () => {
    if (selectedRoundId == null) {
      return;
    }
    const cp = Number.parseInt(cpIdStr, 10);
    if (!Number.isFinite(cp)) {
      setError("ID point de contrôle invalide.");
      return;
    }
    setError(null);
    try {
      const n = numValStr.trim() === "" ? null : Number.parseFloat(numValStr);
      await recordInspectionResult({
        round_id: selectedRoundId,
        checkpoint_id: cp,
        numeric_value: n ?? null,
        comment: resComment.trim() || null,
      });
      setCpIdStr("");
      setNumValStr("");
      setResComment("");
      await loadExecution();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onEnqueueOffline = async () => {
    if (offlineTempId.trim() === "") {
      setError("local_temp_id requis.");
      return;
    }
    setError(null);
    try {
      await enqueueInspectionOffline({
        payload_json: offlinePayload,
        local_temp_id: offlineTempId.trim(),
      });
      setOfflineTempId("");
      await loadExecution();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onRouteAnomalyDi = async (a: InspectionAnomaly) => {
    setError(null);
    try {
      await routeInspectionAnomalyToDi({
        anomaly_id: a.id,
        expected_row_version: a.row_version,
        title: null,
        description: null,
      });
      await loadExecution();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onRouteAnomalyWo = async (a: InspectionAnomaly) => {
    const typeId = Number.parseInt(woTypeIdStr, 10);
    if (!Number.isFinite(typeId)) {
      setError("type_id OT invalide.");
      return;
    }
    setError(null);
    try {
      await routeInspectionAnomalyToWo({
        anomaly_id: a.id,
        expected_row_version: a.row_version,
        type_id: typeId,
        title: null,
      });
      await loadExecution();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onDeferAnomaly = async (a: InspectionAnomaly) => {
    setError(null);
    try {
      await deferInspectionAnomaly({ anomaly_id: a.id, expected_row_version: a.row_version });
      await loadExecution();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onRefreshReliabilitySignals = async () => {
    const d = Number.parseInt(signalWindowDays, 10);
    if (!Number.isFinite(d) || d < 1) {
      setError("Fenêtre (jours) invalide.");
      return;
    }
    setError(null);
    try {
      await refreshInspectionReliabilitySignals({ window_days: d });
      await loadReliabilitySignals();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const selected = templates.find((t) => t.id === selectedId);

  return (
    <ModulePageShell
      icon={ClipboardCheck}
      title="Rondes et checklists"
      description="PRD §6.25 — modèles, versions figées, rondes planifiées."
      bodyClassName="space-y-6 overflow-auto p-4"
    >
      {error ? (
        <div className="rounded-md border border-semantic-danger/40 bg-semantic-danger/10 px-3 py-2 text-sm text-semantic-danger">
          {error}
        </div>
      ) : null}

      <section className="rounded-lg border border-surface-3 bg-surface-1 p-4">
        <h2 className="mb-3 text-sm font-medium text-fg-1">Signaux conformité / RAMS (agrégats)</h2>
        <p className="mb-3 text-xs text-fg-2">
          Fenêtre glissante : avertissements / échecs sur résultats, anomalies ouvertes, couverture
          points (réalisé / planifié par ronde).
        </p>
        <div className="mb-3 flex flex-wrap items-end gap-3">
          <label className="flex flex-col gap-1 text-xs text-fg-2">
            Jours
            <input
              className="rounded border border-surface-3 bg-surface-0 px-2 py-1.5 text-sm text-fg-0"
              value={signalWindowDays}
              onChange={(e) => setSignalWindowDays(e.target.value)}
            />
          </label>
          <button
            type="button"
            className="rounded-md border border-surface-3 px-3 py-1.5 text-xs font-medium text-fg-1 hover:bg-surface-2"
            onClick={() => void onRefreshReliabilitySignals()}
          >
            Recalculer (ins.admin)
          </button>
          <button
            type="button"
            className="rounded-md border border-surface-3 px-3 py-1.5 text-xs font-medium text-fg-1 hover:bg-surface-2"
            onClick={() => void loadReliabilitySignals()}
          >
            Actualiser la liste
          </button>
        </div>
        <div className="overflow-x-auto">
          <table className="w-full text-left text-sm">
            <thead>
              <tr className="border-b border-surface-3 text-fg-2">
                <th className="py-2 pr-4 font-medium">Équipement</th>
                <th className="py-2 pr-4 font-medium">Période</th>
                <th className="py-2 pr-4 font-medium">Warn</th>
                <th className="py-2 pr-4 font-medium">Fail</th>
                <th className="py-2 pr-4 font-medium">Anom. ouv.</th>
                <th className="py-2 font-medium">Couverture</th>
              </tr>
            </thead>
            <tbody>
              {reliabilitySignals.map((s) => (
                <tr key={s.id} className="border-b border-surface-3/60">
                  <td className="py-2 pr-4">{s.equipment_id}</td>
                  <td className="py-2 pr-4 font-mono text-xs">
                    {s.period_start.slice(0, 10)} → {s.period_end.slice(0, 10)}
                  </td>
                  <td className="py-2 pr-4">{s.warning_count}</td>
                  <td className="py-2 pr-4">{s.fail_count}</td>
                  <td className="py-2 pr-4">{s.anomaly_open_count}</td>
                  <td className="py-2">{(s.checkpoint_coverage_ratio * 100).toFixed(1)}%</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        {reliabilitySignals.length === 0 ? (
          <p className="mt-2 text-sm text-fg-2">
            Aucun signal — recalculer avec des données d&apos;inspection.
          </p>
        ) : null}
      </section>

      <section className="rounded-lg border border-surface-3 bg-surface-1 p-4">
        <h2 className="mb-3 text-sm font-medium text-fg-1">Nouveau modèle</h2>
        <div className="flex flex-wrap items-end gap-3">
          <label className="flex flex-col gap-1 text-xs text-fg-2">
            Code
            <input
              className="rounded border border-surface-3 bg-surface-0 px-2 py-1.5 text-sm text-fg-0"
              value={code}
              onChange={(e) => setCode(e.target.value)}
              placeholder="ex. RONDE-A"
            />
          </label>
          <label className="flex min-w-[12rem] flex-col gap-1 text-xs text-fg-2">
            Nom
            <input
              className="rounded border border-surface-3 bg-surface-0 px-2 py-1.5 text-sm text-fg-0"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="Libellé"
            />
          </label>
          <button
            type="button"
            className="rounded-md border border-surface-3 px-3 py-1.5 text-xs font-medium text-fg-1 hover:bg-surface-2"
            onClick={() => void onCreate()}
            disabled={!code.trim() || !name.trim()}
          >
            Créer (v1 + point par défaut)
          </button>
        </div>
      </section>

      <section className="rounded-lg border border-surface-3 bg-surface-1 p-4">
        <div className="mb-3 flex items-center justify-between gap-2">
          <h2 className="text-sm font-medium text-fg-1">Modèles</h2>
          <button
            type="button"
            className="rounded-md border border-surface-3 px-3 py-1.5 text-xs font-medium text-fg-1 hover:bg-surface-2"
            onClick={() => void loadLists()}
          >
            Actualiser
          </button>
        </div>
        <div className="overflow-x-auto">
          <table className="w-full text-left text-sm">
            <thead>
              <tr className="border-b border-surface-3 text-fg-2">
                <th className="py-2 pr-2 font-medium" aria-label="Sel" />
                <th className="py-2 pr-4 font-medium">Code</th>
                <th className="py-2 pr-4 font-medium">Nom</th>
                <th className="py-2 pr-4 font-medium">Version courante</th>
                <th className="py-2 font-medium">Actif</th>
              </tr>
            </thead>
            <tbody>
              {templates.map((row) => (
                <tr key={row.id} className="border-b border-surface-3/60">
                  <td className="py-2 pr-2">
                    <input
                      type="radio"
                      name="tpl"
                      checked={selectedId === row.id}
                      onChange={() => setSelectedId(row.id)}
                    />
                  </td>
                  <td className="py-2 pr-4 font-mono text-xs">{row.code}</td>
                  <td className="py-2 pr-4">{row.name}</td>
                  <td className="py-2 pr-4">{row.current_version_id ?? "—"}</td>
                  <td className="py-2">{row.is_active ? "oui" : "non"}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        {templates.length === 0 ? <p className="mt-2 text-sm text-fg-2">Aucun modèle.</p> : null}
      </section>

      {selectedId != null ? (
        <>
          <section className="rounded-lg border border-surface-3 bg-surface-1 p-4">
            <h2 className="mb-3 text-sm font-medium text-fg-1">Versions — {selected?.code}</h2>
            <div className="overflow-x-auto">
              <table className="w-full text-left text-sm">
                <thead>
                  <tr className="border-b border-surface-3 text-fg-2">
                    <th className="py-2 pr-4 font-medium">#</th>
                    <th className="py-2 pr-4 font-medium">Depuis</th>
                    <th className="py-2 font-medium">Révision</th>
                  </tr>
                </thead>
                <tbody>
                  {versions.map((v) => (
                    <tr key={v.id} className="border-b border-surface-3/60">
                      <td className="py-2 pr-4">{v.version_no}</td>
                      <td className="py-2 pr-4">{v.effective_from ?? "—"}</td>
                      <td className="py-2">{v.row_version}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </section>

          <section className="rounded-lg border border-surface-3 bg-surface-1 p-4">
            <h2 className="mb-3 text-sm font-medium text-fg-1">Points (version courante)</h2>
            <div className="overflow-x-auto">
              <table className="w-full text-left text-sm">
                <thead>
                  <tr className="border-b border-surface-3 text-fg-2">
                    <th className="py-2 pr-4 font-medium">Ordre</th>
                    <th className="py-2 pr-4 font-medium">Code</th>
                    <th className="py-2 font-medium">Type</th>
                  </tr>
                </thead>
                <tbody>
                  {checkpoints.map((c) => (
                    <tr key={c.id} className="border-b border-surface-3/60">
                      <td className="py-2 pr-4">{c.sequence_order}</td>
                      <td className="py-2 pr-4 font-mono text-xs">{c.checkpoint_code}</td>
                      <td className="py-2">{c.check_type}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            {checkpoints.length === 0 ? (
              <p className="mt-2 text-sm text-fg-2">Aucun point pour cette version.</p>
            ) : null}
          </section>

          <section className="rounded-lg border border-surface-3 bg-surface-1 p-4">
            <h2 className="mb-3 text-sm font-medium text-fg-1">Planifier une ronde</h2>
            <div className="flex flex-wrap items-end gap-3">
              <label className="flex flex-col gap-1 text-xs text-fg-2">
                Planifié (ISO, optionnel)
                <input
                  className="rounded border border-surface-3 bg-surface-0 px-2 py-1.5 text-sm text-fg-0"
                  value={scheduleAt}
                  onChange={(e) => setScheduleAt(e.target.value)}
                  placeholder="2026-04-18T10:00:00Z"
                />
              </label>
              <button
                type="button"
                className="rounded-md border border-surface-3 px-3 py-1.5 text-xs font-medium text-fg-1 hover:bg-surface-2"
                onClick={() => void onSchedule()}
              >
                Planifier (statut « scheduled »)
              </button>
            </div>
          </section>
        </>
      ) : null}

      <section className="rounded-lg border border-surface-3 bg-surface-1 p-4">
        <h2 className="mb-3 text-sm font-medium text-fg-1">Rondes récentes</h2>
        <div className="overflow-x-auto">
          <table className="w-full text-left text-sm">
            <thead>
              <tr className="border-b border-surface-3 text-fg-2">
                <th className="py-2 pr-2 font-medium" aria-label="Sel" />
                <th className="py-2 pr-4 font-medium">Modèle</th>
                <th className="py-2 pr-4 font-medium">Version</th>
                <th className="py-2 pr-4 font-medium">Planifié</th>
                <th className="py-2 font-medium">Statut</th>
              </tr>
            </thead>
            <tbody>
              {rounds.map((r) => (
                <tr key={r.id} className="border-b border-surface-3/60">
                  <td className="py-2 pr-2">
                    <input
                      type="radio"
                      name="round"
                      checked={selectedRoundId === r.id}
                      onChange={() => setSelectedRoundId(r.id)}
                    />
                  </td>
                  <td className="py-2 pr-4">{r.template_id}</td>
                  <td className="py-2 pr-4">{r.template_version_id}</td>
                  <td className="py-2 pr-4">{r.scheduled_at ?? "—"}</td>
                  <td className="py-2">{r.status}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        {rounds.length === 0 ? <p className="mt-2 text-sm text-fg-2">Aucune ronde.</p> : null}
      </section>

      {selectedRoundId != null ? (
        <>
          <section className="rounded-lg border border-surface-3 bg-surface-1 p-4">
            <h2 className="mb-3 text-sm font-medium text-fg-1">
              Résultats (ronde #{selectedRoundId})
            </h2>
            <div className="overflow-x-auto">
              <table className="w-full text-left text-sm">
                <thead>
                  <tr className="border-b border-surface-3 text-fg-2">
                    <th className="py-2 pr-4 font-medium">Point</th>
                    <th className="py-2 pr-4 font-medium">Statut</th>
                    <th className="py-2 pr-4 font-medium">Num.</th>
                    <th className="py-2 font-medium">Enregistré</th>
                  </tr>
                </thead>
                <tbody>
                  {results.map((x) => (
                    <tr key={x.id} className="border-b border-surface-3/60">
                      <td className="py-2 pr-4">{x.checkpoint_id}</td>
                      <td className="py-2 pr-4">{x.result_status}</td>
                      <td className="py-2 pr-4">{x.numeric_value ?? "—"}</td>
                      <td className="py-2">{x.recorded_at}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            {results.length === 0 ? (
              <p className="mt-2 text-sm text-fg-2">Aucun résultat.</p>
            ) : null}
          </section>

          <section className="rounded-lg border border-surface-3 bg-surface-1 p-4">
            <h2 className="mb-3 text-sm font-medium text-fg-1">Anomalies</h2>
            <div className="mb-3 flex flex-wrap items-end gap-3">
              <label className="flex flex-col gap-1 text-xs text-fg-2">
                type_id (OT depuis anomalie)
                <input
                  className="rounded border border-surface-3 bg-surface-0 px-2 py-1.5 text-sm text-fg-0"
                  value={woTypeIdStr}
                  onChange={(e) => setWoTypeIdStr(e.target.value)}
                />
              </label>
            </div>
            <div className="overflow-x-auto">
              <table className="w-full text-left text-sm">
                <thead>
                  <tr className="border-b border-surface-3 text-fg-2">
                    <th className="py-2 pr-4 font-medium">Type</th>
                    <th className="py-2 pr-4 font-medium">Gravité</th>
                    <th className="py-2 pr-4 font-medium">Résolution</th>
                    <th className="py-2 pr-4 font-medium">Routage</th>
                    <th className="py-2 font-medium">Actions</th>
                  </tr>
                </thead>
                <tbody>
                  {anomalies.map((a) => {
                    const routable = a.linked_di_id == null && a.linked_work_order_id == null;
                    return (
                      <tr key={a.id} className="border-b border-surface-3/60">
                        <td className="py-2 pr-4">{a.anomaly_type}</td>
                        <td className="py-2 pr-4">{a.severity}</td>
                        <td className="py-2 pr-4">{a.resolution_status}</td>
                        <td className="py-2 pr-4">{a.routing_decision ?? "—"}</td>
                        <td className="py-2">
                          {routable ? (
                            <div className="flex flex-wrap gap-2">
                              <button
                                type="button"
                                className="rounded border border-surface-3 px-2 py-1 text-xs text-fg-1 hover:bg-surface-2"
                                onClick={() => void onRouteAnomalyDi(a)}
                              >
                                DI
                              </button>
                              <button
                                type="button"
                                className="rounded border border-surface-3 px-2 py-1 text-xs text-fg-1 hover:bg-surface-2"
                                onClick={() => void onRouteAnomalyWo(a)}
                              >
                                OT
                              </button>
                              <button
                                type="button"
                                className="rounded border border-surface-3 px-2 py-1 text-xs text-fg-1 hover:bg-surface-2"
                                onClick={() => void onDeferAnomaly(a)}
                              >
                                Reporter
                              </button>
                            </div>
                          ) : (
                            <span className="text-fg-2">
                              {a.linked_di_id != null ? `DI ${a.linked_di_id}` : ""}
                              {a.linked_work_order_id != null ? `OT ${a.linked_work_order_id}` : ""}
                            </span>
                          )}
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>
            {anomalies.length === 0 ? (
              <p className="mt-2 text-sm text-fg-2">Aucune anomalie.</p>
            ) : null}
          </section>

          <section className="rounded-lg border border-surface-3 bg-surface-1 p-4">
            <h2 className="mb-3 text-sm font-medium text-fg-1">
              Saisir un résultat (numérique auto)
            </h2>
            <div className="flex flex-wrap items-end gap-3">
              <label className="flex flex-col gap-1 text-xs text-fg-2">
                ID point
                <input
                  className="rounded border border-surface-3 bg-surface-0 px-2 py-1.5 text-sm text-fg-0"
                  value={cpIdStr}
                  onChange={(e) => setCpIdStr(e.target.value)}
                  placeholder="id checkpoint"
                />
              </label>
              <label className="flex flex-col gap-1 text-xs text-fg-2">
                Valeur
                <input
                  className="rounded border border-surface-3 bg-surface-0 px-2 py-1.5 text-sm text-fg-0"
                  value={numValStr}
                  onChange={(e) => setNumValStr(e.target.value)}
                  placeholder="82.4"
                />
              </label>
              <label className="flex min-w-[10rem] flex-col gap-1 text-xs text-fg-2">
                Commentaire
                <input
                  className="rounded border border-surface-3 bg-surface-0 px-2 py-1.5 text-sm text-fg-0"
                  value={resComment}
                  onChange={(e) => setResComment(e.target.value)}
                />
              </label>
              <button
                type="button"
                className="rounded-md border border-surface-3 px-3 py-1.5 text-xs font-medium text-fg-1 hover:bg-surface-2"
                onClick={() => void onRecordResult()}
              >
                Enregistrer
              </button>
            </div>
          </section>

          <section className="rounded-lg border border-surface-3 bg-surface-1 p-4">
            <h2 className="mb-3 text-sm font-medium text-fg-1">
              File offline (file d&apos;attente locale)
            </h2>
            <div className="mb-3 flex flex-wrap items-end gap-3">
              <label className="flex min-w-[14rem] flex-col gap-1 text-xs text-fg-2">
                payload_json
                <input
                  className="rounded border border-surface-3 bg-surface-0 px-2 py-1.5 font-mono text-xs text-fg-0"
                  value={offlinePayload}
                  onChange={(e) => setOfflinePayload(e.target.value)}
                />
              </label>
              <label className="flex flex-col gap-1 text-xs text-fg-2">
                local_temp_id
                <input
                  className="rounded border border-surface-3 bg-surface-0 px-2 py-1.5 text-sm text-fg-0"
                  value={offlineTempId}
                  onChange={(e) => setOfflineTempId(e.target.value)}
                  placeholder="uuid local"
                />
              </label>
              <button
                type="button"
                className="rounded-md border border-surface-3 px-3 py-1.5 text-xs font-medium text-fg-1 hover:bg-surface-2"
                onClick={() => void onEnqueueOffline()}
              >
                Enqueue
              </button>
            </div>
            <div className="overflow-x-auto">
              <table className="w-full text-left text-sm">
                <thead>
                  <tr className="border-b border-surface-3 text-fg-2">
                    <th className="py-2 pr-4 font-medium">id</th>
                    <th className="py-2 pr-4 font-medium">sync</th>
                    <th className="py-2 font-medium">temp</th>
                  </tr>
                </thead>
                <tbody>
                  {offlineQueue.map((q) => (
                    <tr key={q.id} className="border-b border-surface-3/60">
                      <td className="py-2 pr-4">{q.id}</td>
                      <td className="py-2 pr-4">{q.sync_status}</td>
                      <td className="py-2 font-mono text-xs">{q.local_temp_id}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </section>
        </>
      ) : null}
    </ModulePageShell>
  );
}
