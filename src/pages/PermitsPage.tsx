import { HardHat } from "lucide-react";
import { useCallback, useEffect, useState } from "react";

import { ModulePageShell } from "@/components/layout/ModulePageShell";
import { useSession } from "@/hooks/use-session";
import {
  getLotoCardView,
  listOpenPermitsReport,
  listPermitHandoverLogs,
  listPermitIsolations,
  listPermitSuspensions,
  listPermitTypes,
  listWorkPermits,
  permitComplianceKpi30d,
  recordLotoCardPrint,
  type LotoCardViewRecord,
  type PermitComplianceKpi30dRecord,
  type PermitHandoverLogRecord,
  type PermitIsolationRecord,
  type PermitSuspensionRecord,
  type PermitTypeRecord,
  type WorkPermitRecord,
} from "@/services/permit-service";

export function PermitsPage() {
  const { info: session } = useSession();
  const [types, setTypes] = useState<PermitTypeRecord[]>([]);
  const [permits, setPermits] = useState<WorkPermitRecord[]>([]);
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [selectedIsolationId, setSelectedIsolationId] = useState<number | null>(null);
  const [isolations, setIsolations] = useState<PermitIsolationRecord[]>([]);
  const [cardView, setCardView] = useState<LotoCardViewRecord | null>(null);
  const [kpi30d, setKpi30d] = useState<PermitComplianceKpi30dRecord | null>(null);
  const [openPermits, setOpenPermits] = useState<WorkPermitRecord[]>([]);
  const [suspensions, setSuspensions] = useState<PermitSuspensionRecord[]>([]);
  const [handovers, setHandovers] = useState<PermitHandoverLogRecord[]>([]);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setError(null);
    try {
      const [t, p, kpi, open] = await Promise.all([
        listPermitTypes(),
        listWorkPermits({ limit: 100 }),
        permitComplianceKpi30d(),
        listOpenPermitsReport(),
      ]);
      setTypes(t);
      setPermits(p);
      setKpi30d(kpi);
      setOpenPermits(open);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  const loadLifecycle = useCallback(async (permitId: number) => {
    setError(null);
    try {
      const [s, h] = await Promise.all([
        listPermitSuspensions(permitId),
        listPermitHandoverLogs(permitId),
      ]);
      setSuspensions(s);
      setHandovers(h);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    if (selectedId != null) {
      void loadLifecycle(selectedId);
    } else {
      setSuspensions([]);
      setHandovers([]);
    }
  }, [selectedId, loadLifecycle]);

  useEffect(() => {
    if (selectedId == null) {
      setIsolations([]);
      setSelectedIsolationId(null);
      setCardView(null);
      return;
    }
    let cancelled = false;
    void (async () => {
      try {
        const rows = await listPermitIsolations(selectedId);
        if (!cancelled) {
          setIsolations(rows);
          setSelectedIsolationId(null);
          setCardView(null);
        }
      } catch {
        if (!cancelled) {
          setIsolations([]);
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [selectedId]);

  useEffect(() => {
    if (selectedId == null || selectedIsolationId == null) {
      setCardView(null);
      return;
    }
    let cancelled = false;
    void (async () => {
      try {
        const v = await getLotoCardView(selectedId, selectedIsolationId);
        if (!cancelled) {
          setCardView(v);
        }
      } catch {
        if (!cancelled) {
          setCardView(null);
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [selectedId, selectedIsolationId]);

  return (
    <ModulePageShell
      icon={HardHat}
      title="Permis de travail"
      description="PRD §6.23 — cycle de vie, suspensions et mainlevées."
      bodyClassName="space-y-6 p-4"
    >
      {error ? (
        <div className="rounded-md border border-semantic-danger/40 bg-semantic-danger/10 px-3 py-2 text-sm text-semantic-danger">
          {error}
        </div>
      ) : null}

      {kpi30d ? (
        <section className="rounded-lg border border-surface-3 bg-surface-1 p-4">
          <h2 className="mb-2 text-sm font-medium text-fg-1">Conformité LOTO (30 j.)</h2>
          <p className="text-sm text-fg-2">
            Activations: {kpi30d.activated_count} · Mainlevées à temps:{" "}
            {kpi30d.handed_back_on_time_count}
            {kpi30d.rate != null ? ` · Taux: ${(kpi30d.rate * 100).toFixed(1)} %` : ""}
          </p>
        </section>
      ) : null}

      <section className="rounded-lg border border-surface-3 bg-surface-1 p-4">
        <h2 className="mb-3 text-sm font-medium text-fg-1">Permis ouverts (non expirés)</h2>
        <div className="overflow-x-auto">
          <table className="w-full text-left text-sm">
            <thead>
              <tr className="border-b border-surface-3 text-fg-2">
                <th className="py-2 pr-4 font-medium">Code</th>
                <th className="py-2 pr-4 font-medium">Statut</th>
                <th className="py-2 font-medium">Expire</th>
              </tr>
            </thead>
            <tbody>
              {openPermits.map((row) => (
                <tr key={row.id} className="border-b border-surface-3/60">
                  <td className="py-2 pr-4 font-mono text-xs">{row.code}</td>
                  <td className="py-2 pr-4">{row.status}</td>
                  <td className="py-2">{row.expires_at ?? "—"}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        {openPermits.length === 0 ? (
          <p className="mt-2 text-sm text-fg-2">Aucun permis ouvert.</p>
        ) : null}
      </section>

      <section className="rounded-lg border border-surface-3 bg-surface-1 p-4">
        <h2 className="mb-3 text-sm font-medium text-fg-1">Types de permis</h2>
        <div className="overflow-x-auto">
          <table className="w-full text-left text-sm">
            <thead>
              <tr className="border-b border-surface-3 text-fg-2">
                <th className="py-2 pr-4 font-medium">Code</th>
                <th className="py-2 pr-4 font-medium">Nom</th>
                <th className="py-2 pr-4 font-medium">Durée max (h)</th>
                <th className="py-2 font-medium">Version</th>
              </tr>
            </thead>
            <tbody>
              {types.map((row) => (
                <tr key={row.id} className="border-b border-surface-3/60">
                  <td className="py-2 pr-4 font-mono text-xs">{row.code}</td>
                  <td className="py-2 pr-4">{row.name}</td>
                  <td className="py-2 pr-4">{row.max_duration_hours ?? "—"}</td>
                  <td className="py-2">{row.row_version}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </section>

      <section className="rounded-lg border border-surface-3 bg-surface-1 p-4">
        <div className="mb-3 flex items-center justify-between gap-2">
          <h2 className="text-sm font-medium text-fg-1">Permis</h2>
          <button
            type="button"
            className="rounded-md border border-surface-3 px-3 py-1.5 text-xs font-medium text-fg-1 hover:bg-surface-2"
            onClick={() => void load()}
          >
            Actualiser
          </button>
        </div>
        <div className="overflow-x-auto">
          <table className="w-full text-left text-sm">
            <thead>
              <tr className="border-b border-surface-3 text-fg-2">
                <th className="py-2 pr-2 font-medium" aria-label="Sélection" />
                <th className="py-2 pr-4 font-medium">Code</th>
                <th className="py-2 pr-4 font-medium">Statut</th>
                <th className="py-2 pr-4 font-medium">Type</th>
                <th className="py-2 pr-4 font-medium">Équipement</th>
                <th className="py-2 font-medium">Version</th>
              </tr>
            </thead>
            <tbody>
              {permits.map((row) => (
                <tr key={row.id} className="border-b border-surface-3/60">
                  <td className="py-2 pr-2">
                    <input
                      type="radio"
                      name="permit-select"
                      className="accent-primary"
                      checked={selectedId === row.id}
                      onChange={() => setSelectedId(row.id)}
                      aria-label={`Sélectionner ${row.code}`}
                    />
                  </td>
                  <td className="py-2 pr-4 font-mono text-xs">{row.code}</td>
                  <td className="py-2 pr-4">{row.status}</td>
                  <td className="py-2 pr-4">{row.permit_type_id}</td>
                  <td className="py-2 pr-4">{row.asset_id}</td>
                  <td className="py-2">{row.row_version}</td>
                </tr>
              ))}
            </tbody>
          </table>
          {permits.length === 0 ? (
            <p className="mt-2 text-sm text-fg-2">Aucun permis pour le moment.</p>
          ) : null}
        </div>
      </section>

      {selectedId != null ? (
        <section className="grid gap-4 rounded-lg border border-surface-3 bg-surface-1 p-4 md:grid-cols-2">
          <div className="md:col-span-2">
            <h2 className="mb-2 text-sm font-medium text-fg-1">Points d&apos;isolation LOTO</h2>
            {isolations.length === 0 ? (
              <p className="text-sm text-fg-2">Aucun point d&apos;isolation.</p>
            ) : (
              <ul className="space-y-2">
                {isolations.map((iso) => (
                  <li key={iso.id} className="flex flex-wrap items-center gap-2 text-sm">
                    <input
                      type="radio"
                      name="iso-select"
                      className="accent-primary"
                      checked={selectedIsolationId === iso.id}
                      onChange={() => setSelectedIsolationId(iso.id)}
                      aria-label={`Isolation ${iso.isolation_point}`}
                    />
                    <span className="text-fg-1">{iso.isolation_point}</span>
                    <span className="text-fg-2">({iso.energy_type})</span>
                    {iso.lock_number ? (
                      <span className="font-mono text-xs text-fg-2">
                        Cadenas: {iso.lock_number}
                      </span>
                    ) : null}
                  </li>
                ))}
              </ul>
            )}
            {cardView ? (
              <div className="mt-4 rounded border border-surface-3 bg-surface-0 p-3 font-mono text-xs leading-relaxed text-fg-1">
                <div>Permis: {cardView.permit_code}</div>
                <div>Équipement: {cardView.equipment_label}</div>
                <div>Énergie: {cardView.energy_type}</div>
                <div>
                  Point: #{cardView.isolation_id} — {cardView.isolation_point}
                </div>
                {cardView.lock_number ? <div>Cadenas: {cardView.lock_number}</div> : null}
                <div>Vérificateur: {cardView.verifier_signature ?? "—"}</div>
                <div>Expiration: {cardView.expires_at ?? "—"}</div>
                <button
                  type="button"
                  className="mt-3 rounded-md border border-surface-3 px-2 py-1 text-xs font-medium text-fg-1 hover:bg-surface-2"
                  disabled={!session?.user_id || selectedIsolationId == null}
                  onClick={() => {
                    if (
                      session?.user_id == null ||
                      selectedId == null ||
                      selectedIsolationId == null
                    ) {
                      return;
                    }
                    const printedById = session.user_id;
                    void (async () => {
                      try {
                        await recordLotoCardPrint({
                          permit_id: selectedId,
                          isolation_id: selectedIsolationId,
                          printed_by_id: printedById,
                        });
                        await load();
                      } catch (e) {
                        setError(e instanceof Error ? e.message : String(e));
                      }
                    })();
                  }}
                >
                  Enregistrer impression
                </button>
              </div>
            ) : null}
          </div>
          <div>
            <h2 className="mb-2 text-sm font-medium text-fg-1">
              Suspensions (permit #{selectedId})
            </h2>
            <ul className="space-y-2 text-sm text-fg-2">
              {suspensions.length === 0 ? (
                <li>Aucune suspension.</li>
              ) : (
                suspensions.map((s) => (
                  <li key={s.id} className="rounded border border-surface-3/50 p-2">
                    <span className="text-fg-1">{s.suspended_at}</span> — {s.reason}
                    {s.reinstated_at ? (
                      <span className="mt-1 block text-xs">Réintégration: {s.reinstated_at}</span>
                    ) : null}
                  </li>
                ))
              )}
            </ul>
          </div>
          <div>
            <h2 className="mb-2 text-sm font-medium text-fg-1">Mainlevées / passations</h2>
            <ul className="space-y-2 text-sm text-fg-2">
              {handovers.length === 0 ? (
                <li>Aucune entrée.</li>
              ) : (
                handovers.map((h) => (
                  <li key={h.id} className="rounded border border-surface-3/50 p-2">
                    <span className="text-fg-1">
                      {h.handed_from_role} → {h.handed_to_role}
                    </span>
                    <span className="mt-1 block text-xs">{h.signed_at}</span>
                  </li>
                ))
              )}
            </ul>
          </div>
        </section>
      ) : null}
    </ModulePageShell>
  );
}
